@0x90d5fb2b0b8b5f13;

using import "value.capnp".Primitive;

enum Type {
    unit @0;
    bool @1;
    int @2;
    float @3;
    string @4;
    buffer @5;
    # non-primitive types
    list @6;
    tuple @7;
    record @8;
}

struct CompileError {
    summary @0 :Text;
}

struct Symbol {
    name @0 :Text;
    disam @1 :UInt32;
}

struct Case {
    expr @0 :Expr;
    union {
        default @1 :Void;
        tag @2 :Text; # for enum cases
        listCons @3 :Void; 
        listEmpty @4 :Void;
        tuple @5 :UInt64;
        record :group {
            keys @6 :List(Text);
            exact @7 :Bool;
        }
        eq @8 :Primitive;
        of @9 :Type; # TODO: Revisit
    }
}

struct Expr {
    struct Apply {
        value @0 :Expr;
        union {
            pos @1 :Void;
            key @2 :Text;
            varPos @3 :Void;
            varKey @4 :Void;
        }
    }

    struct Param {
        symbol @0 :Symbol;
        union {
            # Not not generated by ast, but for builtins
            pos @1 :Void;
            named @2 :Text; # fn foo(a) will be named argument "a" with symbol a
            optional @3 :Text; # fn(a?) will be optional argument "a"
            varPos @4 :Void; # fn(*a) will be varpos argument /w symbol "a"
            varKey @5 :Void; # fn(**a) will be varkeys argument a
        }
    }

    struct Binds {
        struct Bind {
            symbol @0 :Symbol;
            value @1 :Expr;
        }
        binds @0 :List(Bind);
        # for non-recursive binds 
        # later symbols depend on earlier symbols
        # recursive binds they can depend
        # in any order
        rec @1 :Bool;
    }

    union {
        id @0 :Symbol;
        literal @1 :Primitive;
        let :group {
            binds @2 :Binds;
            body @3 :Expr;
        }
        lam :group {
            params @4 :List(Param);
            body @5 :Expr;
        }
        app :group {
            lam @6 :Expr;
            args @7 :List(Apply);
        }
        invoke @8 :Expr;
        match :group {
            expr @9 :Expr;
            bindTo @10 :Symbol;
            cases @11 :List(Case);
        }
        # these are meant
        # for use in the prelude
        # and standard libraries only!
        inlineBuiltin :group {
            op @12 :Text;
            args @13 :List(Expr);
        }
        error @14 :CompileError;
    }
}