# GenServer

The counter from the last tutorial had three parts: an initial state (`0`), a function that handles messages and computes new state, and a recursive loop that carries state forward. Every stateful process on the BEAM has these same three parts. GenServer is the built-in abstraction for this pattern.

Start `juice box` and define one:

```
box> const Counter = {
  init: () => ({ count: 0 }),
  handleCall: (msg, state) => {
    if (msg === "increment") {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    } else if (msg === "get") {
      return { reply: state.count, state: state }
    }
  }
}
counter
```

Juice compiled `Counter` into a BEAM module and loaded it into the running VM. Now start it:

```
box> const pid = GenServer.start(Counter)
<0.84.0>
```

A process, just like before — but you didn't write `spawn` or `receive`. Send it some messages:

```
box> GenServer.call(pid, "increment")
1
box> GenServer.call(pid, "increment")
2
box> GenServer.call(pid, "get")
2
```

`GenServer.call` sent a message and waited for the reply. The object `Counter` maps directly to the three parts from before:

- `init` replaces the initial argument — it returns the starting state
- `handleCall` replaces the `receive`/`if` chain — it takes a message and the current state
- The recursive loop is gone — GenServer calls your handler for each message and threads the state through automatically

Look at what `handleCall` returns:

```js
return { reply: next.count, state: next }
```

`reply` is the value sent back to the caller — what `GenServer.call` returns. `state` is what the state should be for the next message. This is the same idea as `counter(count + 1)` from the manual version — you hand back what comes next. Nothing is mutated. The GenServer takes your returned state and carries it forward.

## The callbacks

A GenServer is an object with up to three callbacks:

- `init` — **required**. Called once when the server starts. Takes no arguments, returns the initial state. Every GenServer needs a starting state.
- `handleCall` — optional. Handles synchronous messages sent with `GenServer.call`. Takes `(msg, state)`, returns `{ reply: value, state: newState }`. The caller blocks until the reply comes back.
- `handleCast` — optional. Handles async messages sent with `GenServer.cast`. Takes `(msg, state)`, returns `{ state: newState }`. No reply — fire and forget.

If you omit `handleCall` or `handleCast`, the GenServer still starts — it just ignores those types of messages. A GenServer that only needs async messages can skip `handleCall` entirely, and vice versa.

## call vs cast

`GenServer.call` is synchronous — the caller blocks until the server replies. Sometimes you want to send a message without waiting. That's `GenServer.cast`.

Start a fresh `juice box` and add a reset operation:

```
box> const Counter = {
  init: () => ({ count: 0 }),
  handleCall: (msg, state) => {
    if (msg === "increment") {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    } else if (msg === "get") {
      return { reply: state.count, state: state }
    }
  },
  handleCast: (msg, state) => {
    if (msg === "reset") {
      return { state: { count: 0 } }
    }
  }
}
counter
box> const pid = GenServer.start(Counter)
<0.84.0>
box> GenServer.call(pid, "increment")
1
box> GenServer.call(pid, "increment")
2
box> GenServer.call(pid, "increment")
3
box> GenServer.cast(pid, "reset")
ok
box> GenServer.call(pid, "get")
0
```

`GenServer.cast(pid, "reset")` sent the message and returned `ok` immediately — no waiting, no reply. The counter processed the reset before the next `call` arrived, so `get` returned `0`.

`handleCall` returns `{ reply, state }` because the caller is waiting for an answer. `handleCast` returns just `{ state }` — there's nobody to reply to. Think of `call` as a function call and `cast` as dropping a note in a mailbox.

## Pattern matching

The counter's `handleCall` uses `if/else if` to dispatch on message type. That works, but Erlang has a more natural way to handle this — pattern matching. Juice exposes it through `match()`.

```
box> const x = match("hello",
  "hello", () => "matched!",
  "world", () => "nope"
)
"matched!"
```

`match` takes a value and a list of pattern/handler pairs. It tries each pattern in order and runs the first handler that matches. Strings match as atoms, just like message names in GenServer.

It works as an expression — you can assign the result or return it from a function:

```
box> const describe = (status) => {
  return match(status,
    "ok", () => "all good",
    "error", () => "something broke",
    _, (s) => "unknown: " + s
  )
}
box> describe("ok")
"all good"
box> describe("wat")
"unknown: wat"
```

`_` is the wildcard — it matches anything and passes the value to the handler.

Arrays are tuples on the BEAM, and `match` works with them too. Use `_` in a tuple position to capture that element:

```
box> const handle = (msg) => {
  return match(msg,
    ["ok", _], (result) => "success: " + result,
    ["error", _], (reason) => "failed: " + reason,
    _, () => "unknown"
  )
}
box> handle(["ok", "done"])
"success: done"
box> handle(["error", "timeout"])
"failed: timeout"
```

Each `_` in a tuple pattern becomes a variable — bound left to right to the handler's parameters. `["ok", _]` matches any two-element tuple starting with `"ok"`, and the handler gets the second element as `result`.

### Pattern matching in GenServer

GenServer callbacks can use the same idea. Instead of a function with `if/else if`, you can write `handleCall` as an object where each key is a message pattern:

```
box> const Counter = {
  init: () => ({ count: 0 }),
  handleCall: {
    increment: (state) => {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    },
    get: (state) => ({ reply: state.count, state: state })
  }
}
counter
box> const pid = GenServer.start(Counter)
<0.84.0>
box> GenServer.call(pid, "increment")
1
box> GenServer.call(pid, "get")
2
```

Each key becomes a separate Erlang function clause — the BEAM matches the message directly instead of testing equality in a chain of `if` blocks. The handler receives just `(state)` since the message is already matched by the key.

`handleCast` works the same way:

```javascript
handleCast: {
    reset: (state) => ({ state: { count: 0 } })
}
```

Both forms — function with `if/else if` and object dispatch — produce working GenServers. Object dispatch is more concise and compiles to better Erlang.

## Under the hood

You can see what Juice generates with `--emit-erl`. Save the counter as `counter.ts`:

```js
// counter.ts
const Counter = {
  init: () => ({ count: 0 }),
  handleCall: (msg, state) => {
    if (msg === "increment") {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    } else if (msg === "get") {
      return { reply: state.count, state: state }
    }
  }
}

const pid = GenServer.start(Counter)
console.log(GenServer.call(pid, "increment"))
console.log(GenServer.call(pid, "increment"))
console.log(GenServer.call(pid, "get"))
```

```
$ juice counter.ts --emit-erl
-module(counter).
-behaviour(gen_server).
-export([main/0, init/1, handle_call/3, handle_cast/2, handle_info/2]).

init(_Args) ->
    {ok, #{count => 0}}.

handle_call(Msg, _From, State) ->
    ...
```

The object literal became callback functions, `GenServer.start` became `gen_server:start_link`, and `{ reply, state }` became Erlang's `{reply, Reply, NewState}` tuple. The counter is 17 lines of JavaScript. The generated Erlang is about 40 — mostly plumbing that Juice writes for you.

A GenServer is a process with a protocol. You define `init` for the starting state, `handleCall` for synchronous requests, and `handleCast` for async messages. The GenServer handles the loop, the mailbox, and the state threading. You focus on what each message means.

But what happens when a GenServer crashes?
