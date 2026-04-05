# Hello World

```js
// hello.ts
console.log("hello, world")
```

```
$ juice run hello.ts
hello, world
```

Let's walk through exactly what happened here. Juice compiles your TypeScript to Erlang source, then hands that off to `erlc` which compiles it into BEAM bytecode:

```
hello.ts → hello.erl → hello.beam
```

You can see the generated Erlang with `--emit-erl`:

```
$ juice hello.ts --emit-erl
-module(hello).
-export([main/0]).

main() ->
    io:format("hello, world~n").
```

The `juice run` command compiles and then executes the `.beam` file in a single step. But you can also do this yourself. Run `juice hello.ts` to just compile, then run the `.beam` directly:

```
$ erl -noshell -s hello main -s init stop
hello, world
```

It's worth breaking down this `erl` command because it will come up again as the examples get more interesting:

- `erl` starts the BEAM virtual machine
- `-noshell` runs without an interactive prompt
- `-s hello main` calls the function `main()` in the module `hello`
- `-s init stop` shuts down the VM after execution

That last flag is the interesting one. Without it, the BEAM stays running — because the BEAM is designed to run forever. It's not a program executor like Node or Python where a script runs and exits. It's a runtime for long-lived concurrent systems. Every piece of code runs inside a lightweight **process**, and the VM is built to keep those processes alive, supervised, and talking to each other.

For this hello world example, there's nothing to keep alive, so we tell it to stop. But as we get into processes and message passing, you'll see why "the VM stays running" is a feature, not a bug.
