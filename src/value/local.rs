use capnp::OutputSegments;

use super::storage::{
    DataRef, 
    ObjectRef, ObjPointer, Storage,
    StorageError
};
use super::ValueReader;
use super::allocator::{SegmentAllocator, AllocHandle, Segment, SegmentMut};

// The local object storage table

pub struct LocalObjectStorage<ObjAlloc : SegmentAllocator, DataAlloc : SegmentAllocator> {
    obj_alloc: ObjAlloc,
    data_alloc: DataAlloc,
}

impl<ObjAlloc, DataAlloc> LocalObjectStorage<ObjAlloc, DataAlloc> 
        where ObjAlloc : SegmentAllocator, DataAlloc: SegmentAllocator {
    fn get_data<'s>(&'s self, handle : AllocHandle)
                -> Result<LocalDataRef<'s, DataAlloc>, StorageError> {
        let seg = unsafe {
            // once things have been inserted, you can only get them mutably
            // so this is actually safe to slice into this handle
            let len = self.data_alloc.slice(handle, 0, 1)?.as_slice()[0].to_le();
            self.data_alloc.slice(handle, 1, len - 1)?
        };
        Ok(LocalDataRef { handle, seg })
    }
}

impl<ObjAlloc, DataAlloc> Storage for LocalObjectStorage<ObjAlloc, DataAlloc> 
        where ObjAlloc : SegmentAllocator, DataAlloc: SegmentAllocator {
    type EntryRef<'s> where ObjAlloc : 's, DataAlloc : 's = 
                        LocalEntryRef<'s, ObjAlloc, DataAlloc>;
    type ValueRef<'s> where DataAlloc : 's, ObjAlloc : 's = LocalDataRef<'s, DataAlloc>;

    fn alloc<'s>(&'s self) -> Result<Self::EntryRef<'s>, StorageError> {
        let handle : AllocHandle = self.obj_alloc.alloc(2)?;
        Ok(LocalEntryRef {
            handle, store: self
        })
    }
    fn get<'s>(&'s self, ptr: ObjPointer) -> Result<Self::EntryRef<'s>, StorageError> {
        Ok(LocalEntryRef {
            handle: ptr.unwrap(), store: self
        })
    }

    fn insert<'s>(&'s self, val : ValueReader<'_>) -> Result<Self::ValueRef<'s>, StorageError> {
        let mut builder = capnp::message::Builder::new_default();
        builder.set_root_canonical(val).unwrap();

        let seg = if let OutputSegments::SingleSegment(s) = builder.get_segments_for_output() {
            s[0]
        } else {
            panic!("Should only have a single segment")
        };
        // allocate a single segment
        let len = ((seg.len() + 7) / 8) as u64;
        let handle= self.data_alloc.alloc(len + 1)?;
        // This is safe since no once else can have sliced the memory yet
        let mut hdr_slice = unsafe { self.data_alloc.slice_mut(handle, 0, 1)? };
        let mut slice = unsafe { self.data_alloc.slice_mut(handle, 1, len)? };

        // set header
        hdr_slice.as_slice_mut()[0] = len + 1;
        let raw_dest = slice.as_raw_slice_mut();
        raw_dest.clone_from_slice(seg);
        self.get_data(handle)
    }
}

pub struct LocalEntryRef<'s, ObjAlloc: SegmentAllocator, DataAlloc: SegmentAllocator> {
    handle: AllocHandle,
    // a reference to the original memory object
    store: &'s LocalObjectStorage<ObjAlloc, DataAlloc>
}

impl<'s, ObjAlloc, DataAlloc> ObjectRef<'s> for LocalEntryRef<'s, ObjAlloc, DataAlloc>
                where ObjAlloc : SegmentAllocator, DataAlloc : SegmentAllocator {
    type ValueRef = LocalDataRef<'s, DataAlloc>;

    fn ptr(&self) -> ObjPointer {
        ObjPointer::from(self.handle)
    }

    fn get_value(&self) -> Result<Self::ValueRef, StorageError> {
        let alloc = &self.store.obj_alloc;
        unsafe {
            let seg = alloc.slice(self.handle, 0, 2)?;
            let s : &[u64; 2] = seg.as_slice().try_into().unwrap();
            self.store.get_data(s[0])
        }
    }


    // Will push a result value over a thunk value
    fn push_result(&self, val: Self::ValueRef) {
        let alloc = &self.store.obj_alloc;
        unsafe {
            let mut seg = alloc.slice_mut(self.handle, 0, 2).unwrap();
            let s : &mut [u64; 2] = seg.as_slice_mut().try_into().unwrap();
            s[1] = s[0];
            s[0] = val.handle
        }
    }
    // Will restore the old thunk value
    // and return the current value (if it exists)
    fn pop_result(&self) {
        let alloc = &self.store.obj_alloc;
        unsafe {
            let mut seg = alloc.slice_mut(self.handle, 0, 2).unwrap();
            let s : &mut [u64; 2] = seg.as_slice_mut().try_into().unwrap();
            s[0] = s[1];
            s[1] = 0;
        }
    }
}


pub struct LocalDataRef<'s, Alloc: SegmentAllocator + 's> {
    handle: AllocHandle,
    seg: Alloc::Segment<'s>
}

impl<'s, Alloc: SegmentAllocator> DataRef<'s> for LocalDataRef<'s, Alloc> {
    fn reader<'r>(&'r self) -> ValueReader<'r> {
        let slice = self.seg.as_slice();
        // convert to u8 slice
        let data = unsafe {
            let n_bytes = slice.len() * std::mem::size_of::<u64>();
            std::slice::from_raw_parts(slice.as_ptr() as *const u8, n_bytes)
        };
        let any_ptr = capnp::any_pointer::Reader::new(
            capnp::private::layout::PointerReader::get_root_unchecked(&data[0])
        );
        any_ptr.get_as().unwrap()
    }
}