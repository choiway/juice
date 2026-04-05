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

## Process utilities

Juice exposes a `Process` module for managing processes beyond the basics.

### Process.info

On the BEAM, a process doesn't have a state object you can inspect from the outside. State lives in the arguments of the recursive function call — it's private to the process. To make state visible, you build it into the message protocol: the process responds to a query.

Here's a counter that tracks its own state and can report it back:

```
box> const counter = (count) => {
  receive((msg) => {
    if (msg === "inc") {
      counter(count + 1)
    } else if (msg === "dec") {
      counter(count - 1)
    } else {
      send(msg, count)
      counter(count)
    }
  })
}
box> const pid = spawn(() => { counter(0) })
<0.84.0>
box> send(pid, "inc")
box> send(pid, "inc")
box> send(pid, "inc")
box> send(pid, self())
3
```

When the process receives a pid instead of a command, it sends its current count back. The caller gets the state by asking for it — there's no way to peek inside a process without its cooperation.

`Process.info(pid)` gives you VM-level metadata — whether the process is alive, what it's doing, how many messages are queued:

```
box> Process.info(pid)
```

Useful fields:

- `status` — `waiting` (blocked in receive), `running`, `runnable`
- `message_queue_len` — messages sitting in the mailbox, not yet received
- `messages` — the actual queued messages
- `links` — other processes linked to this one (if one crashes, the linked process gets notified)

This is a debugging tool, not a way to read state. If a process seems stuck, `Process.info` tells you whether it's alive, whether messages are piling up, and whether it's actually waiting in `receive`.

### Process.register and Process.whereis

Every process gets a pid when it's spawned, but pids are temporary — they change every time you restart a process. `Process.register` gives a process a name so other processes can find it without knowing the pid:

```
box> const loop = () => { receive((msg) => { console.log("got: " + msg); loop() }) }
box> const pid = spawn(loop)
<0.84.0>
box> Process.register("greeter", pid)
true
```

Now any process can look it up by name:

```
box> const found = Process.whereis("greeter")
<0.84.0>
box> send(found, "hello")
got: hello
```

`Process.whereis` returns the pid for a registered name, or `undefined` if nothing is registered under that name. This is how services find each other on the BEAM — instead of passing pids around, you register a name and let callers look it up.

### Process.exit

As shown earlier, `Process.exit(pid, reason)` kills a process:

```
box> Process.exit(pid, "kill")
true
```

The reason `"kill"` is special — it's an unconditional kill that can't be trapped. Other reasons (like `"normal"` or `"shutdown"`) can be intercepted by processes that set `trap_exit` to true, which is how supervisors know when their children crash.
