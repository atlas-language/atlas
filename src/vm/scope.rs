use crate::value::storage::{
    Storage, ObjectRef,
};

use super::op::{OpAddr, OpCount, ObjectID, CodeReader, DestReader, Dependent};
use super::ExecError;

use deadqueue::unlimited::Queue;
use std::collections::HashMap;
use slab::Slab;
use std::cell::RefCell;


// An execqueue manages the execution of a particular
// code block by tracking dependencies
// It needs to be shared among all ongoing coroutines
// being executed
pub struct ExecQueue {
    // The queue of operations that are
    // ready to execute
    queue : Queue<OpAddr>,
    // map from op to number of dependencies
    // left to be satisfied.
    waiting : RefCell<HashMap<OpAddr, OpCount>>,
}

impl ExecQueue {
    pub fn new() -> Self {
        Self { 
            queue: Queue::new(), 
            waiting: RefCell::new(HashMap::new())
        }
    }

    pub async fn next_op(&self) -> OpAddr {
        self.queue.pop().await
    }

    // Will complete a particular operation, getting each of the
    // dependents and notifying them that a dependency has been completed
    pub fn complete(&self, dest: DestReader<'_>, code: CodeReader<'_>) -> Result<(), ExecError> {
        let deps = dest.get_used_by()?;
        for d in deps.iter() {
            self.dep_complete_for(d, code)?;
        }
        Ok(())
    }

    // notify the execution queue that a dependency
    // for a given op was completed. This will release the
    // operation into the queue if all the dependencies have been
    // completed. If this is the first time the given operation
    // has a dependency complete, we read the operation and determine
    // the number of dependencies it has.
    fn dep_complete_for(&self, op: OpAddr, code: CodeReader<'_>) -> Result<(), ExecError> {
        let opr = code.get_ops()?.get(op);
        let mut w = self.waiting.borrow_mut();
        match w.get_mut(&op) {
            Some(r) => {
                *r = *r  - 1;
                if *r == 0 {
                    // release into the queue
                    w.remove(&op);
                    self.queue.push(op);
                }
            },
            None => {
                // this is the first time this op
                // is being listed as dependency complete, find the number of dependents
                let deps = opr.num_deps()?;
                if deps > 1 {
                    w.insert(op, deps - 1);
                } else {
                    self.queue.push(op);
                }
            }
        }
        Ok(())
    }
}

pub struct Reg<'sc, S: Storage + 'sc> {
    value: S::EntryRef<'sc>,
    remaining_uses: Option<u16>, // None if this is a lifted allocation
}


// Registers manages a map from data id --> underlying data
// as well as the consumption of data

// From the autoside perspective, this structure should *appear*
// as if it is atomic, so all methods take &
// (so &Registers can be shared among multiple ongoing operations). 
pub struct Registers<'s, S: Storage + 's> {
    // slab-allocated registers
    regs : RefCell<Slab<Reg<'s, S>>>,
    // map from ObjectID to the slab register key
    reg_map: RefCell<HashMap<ObjectID, usize>>,
    store: &'s S
}

impl<'s, S: Storage> Registers<'s, S> {
    pub fn new(store: &'s S) -> Self {
        Self {
            regs: RefCell::new(Slab::new()),
            reg_map: RefCell::new(HashMap::new()),
            store
        }
    }


    // Will allocate an initializer for a given object
    // This will reuse an earlier allocation if the allocation has
    // been lifted, or create a new allocation if not.
    // Note that you still need to call *set_object* in order
    // to set this particular ObjectID, this just manages creating the allocation
    // in a manner that handles recursive definitions
    pub fn alloc_entry(&self, d: ObjectID) -> Result<S::EntryRef<'s>, ExecError> {
        let mut reg_map = self.reg_map.borrow_mut();
        let reg_idx = reg_map.get(&d).map(|x| *x);
        match reg_idx {
            Some(idx) => {
                // use (and remove) the earlier allocation
                let mut regs= self.regs.borrow_mut();
                if let Some(_) = regs.get(idx).ok_or(ExecError{})?.remaining_uses {
                    // If this register has already been allocated this
                    // is an improper reuse!
                    return Err(ExecError{})
                }
                let reg = regs.remove(idx);
                reg_map.remove(&d);
                // return the underlying entry
                Ok(reg.value)
            },
            None => Ok(self.store.alloc()?)
        }
    }

    // Will set a particular ObjectID to a given entry value, as well as
    // a number of uses for this data until the register should be discarded
    pub fn set_object(&self, d: DestReader<'_>, e: S::EntryRef<'s>) -> Result<(), ExecError> {
        // If there is a lifting allocation, that mapping
        // should have been removed using alloc_entry.
        // To ensure that is the case, we error if there is a mapping
        let mut regs = self.regs.borrow_mut();
        let mut reg_map = self.reg_map.borrow_mut();
        let id = d.get_id();
        let uses = d.get_used_by()?.len() as u16;
        let key = regs.insert(Reg{ value: e, remaining_uses: Some(uses) });
        reg_map.insert(id, key);
        Ok(())
    }

    // Will get an entry, either (1) reducing the remaining uses
    // or (2) lifting the allocation
    pub fn consume(&self, d: ObjectID) -> Result<S::EntryRef<'s>, ExecError> {
        let mut reg_map = self.reg_map.borrow_mut();
        let mut regs= self.regs.borrow_mut();

        let reg_idx = reg_map.get(&d).map(|x|*x);
        match reg_idx {
        None => {
            // Create a new lifting allocation, insert a copy into the registers
            // and also return it here.
            let entry = self.store.alloc()?;
            let entry_ret = self.store.get(entry.ptr())?;
            let key = regs.insert(Reg{ value: entry, remaining_uses: None });
            reg_map.insert(d, key);
            Ok(entry_ret)
        },
        Some(idx) => {
            // There already exists an allocation
            let reg = regs.get_mut(idx).ok_or(ExecError {})?;
            let entry = match &mut reg.remaining_uses {
                Some(uses) => {
                    *uses = *uses - 1;
                    if *uses == 0 {
                        // remove the entry and return the underlying ref
                        let reg = regs.remove(idx);
                        reg_map.remove(&d);
                        reg.value
                    } else {
                        // get a new version of the same reference
                        // from the storage
                        self.store.get(reg.value.ptr())?
                    }
                },
                None => self.store.get(reg.value.ptr())?
            };
            Ok(entry)
        }
        }
    }
}

pub fn populate<'s, S : Storage>(regs: &Registers<'s, S>, queue: &ExecQueue, code: CodeReader<'_>, 
                    args: Vec<S::EntryRef<'s>>) 
                    -> Result<(), ExecError> {
    // setup the constants values
    for c in code.get_constants()?.iter() {
        regs.set_object(c.get_dest()?, regs.store.get(c.get_ptr().into())?)?;
        queue.complete(c.get_dest()?, code)?;
    }
    // setup the argument values
    for (e, dest) in args.into_iter().zip(code.get_params()?.iter()) {
        regs.set_object(dest, e)?;
        queue.complete(dest.reborrow(), code)?;
    }
    Ok(())
}