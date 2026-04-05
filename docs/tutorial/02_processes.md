# Processes

Juice adds three keywords that don't exist in JavaScript: `spawn`, `receive`, and `send`. These map directly to BEAM primitives for creating processes and passing messages between them.

- `spawn(fn)` — creates a new BEAM process that runs `fn`, returns its pid
- `send(pid, msg)` — sends a message to a process's mailbox
- `receive(fn)` — blocks until a message arrives, then calls `fn` with it

Juice has an interactive shell. Start it with `juice box`:

```
$ juice box
```

Define a function:

```
box> const greet = () => { console.log("hello") }
```

Call it:

```
box> greet()
hello
```

Now pass the same function to `spawn`:

```
box> const pid = spawn(greet)
hello
<0.84.0>
```

`spawn` created a new BEAM process, ran `greet` inside it, and returned the process ID — a pid. The function ran, printed "hello", and the process exited.

The pid `<0.84.0>` is an address. But this process is already gone — it did its work and disappeared. To make a process that sticks around, give it something to wait for:

```
box> const listener = () => { receive((msg) => { console.log("got: " + msg) }) }
box> const pid2 = spawn(listener)
<0.85.0>
```

Nothing printed. The process is alive and waiting inside `receive` for a message. Send it one using the pid:

```
box> send(pid2, "hello")
got: hello
```

The process received `"hello"`, ran the callback, and printed `"got: hello"`.

But try sending a second message:

```
box> send(pid2, "bye")
bye
```

No `"got: bye"` — just the raw message echoed back. That's because `receive` only handles **one message**. After the first message arrived, the callback ran, the function returned, and the process exited. By the time you sent `"bye"`, there was nobody listening.

To make a process that handles multiple messages, call the function again at the end of the callback. Start a fresh `juice box` session:

```
box> const loop = () => { receive((msg) => { console.log("got: " + msg); loop() }) }
box> const pid = spawn(loop)
<0.84.0>
box> send(pid, "hello")
got: hello
box> send(pid, "bye")
got: bye
box> send(pid, "still here")
got: still here
```

The `loop()` call at the end of the callback puts the process right back into `receive`, waiting for the next message. This is how long-lived processes work on the BEAM — not with while loops, but with recursive functions.

When you're done with a process, kill it with `Process.exit`:

```
box> Process.exit(pid, "kill")
true
box> send(pid, "hello?")
hello?
```

No `"got: hello?"` — the message just echoes back. The process is gone.

This is the core of the BEAM: a process is a function with its own mailbox and address. You create them with `spawn`, talk to them with `send`, and they listen with `receive`.
