# Interactive Runtime: Supervised Projects and Multi-Node

## Quick example

Start a supervised counter and interact with it:

```javascript
// examples/supervision_named.ts
const Counter = {
  init: () => ({ count: 0 }),
  handleCall: (msg, state) => {
    if (msg === "increment") {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    } else if (msg === "get") {
      return { reply: state.count, state: state }
    } else if (msg === "boom") {
      throw new Error("crash!")
    }
  }
}

const sup = Supervisor.start({
  strategy: "one_for_one",
  children: [
    { id: "counter", start: () => GenServer.start(Counter, { name: "counter" }) }
  ]
})
```

```
$ juice start examples/supervision_named.ts
Starting supervision_named...
juice> GenServer.call("counter", "increment")
1
juice> GenServer.call("counter", "increment")
2
juice> GenServer.call("counter", "boom")
juice> GenServer.call("counter", "get")
0  ← supervisor restarted with fresh state
```

Two nodes talking to each other:

```
# Terminal 1
$ juice start examples/supervision_named.ts --name node1
juice@node1> GenServer.call("counter", "increment")
1

# Terminal 2
$ juice connect node1@mymachine
Connected to node1@mymachine
juice@node1> GenServer.call("counter", "get")
1  ← reading state from node1's GenServer
```

---

## Commands

### `juice start <file>`

Compiles the file, starts a persistent BEAM VM with the supervision tree running, then drops into an interactive REPL. Processes stay alive across REPL evaluations.

```
juice start examples/supervision_named.ts
juice start examples/supervision_named.ts --name node1
```

The `--name` flag starts the VM as a named distributed Erlang node, enabling multi-node communication.

### `juice connect <node@host>`

Connects to an already-running Juice node from a separate terminal. Expressions are evaluated on the remote node via `rpc:call`, so registered process names resolve on the remote side.

```
juice connect node1@mymachine
```

Uses a fixed cookie (`juice`) so both sides authenticate automatically.

---

## Named GenServers

By default, `GenServer.start(Counter)` creates an anonymous process accessible only by pid. Passing a `name` option registers the process under that atom, making it discoverable from the REPL or from other nodes.

```javascript
GenServer.start(Counter, { name: "counter" })
```

Compiles to:

```erlang
gen_server:start_link({local, counter}, ?MODULE, [], [])
```

Once named, you can call it by name instead of pid:

```javascript
GenServer.call("counter", "increment")  // no pid needed
```

Named GenServers also work in supervised children:

```javascript
Supervisor.start({
  strategy: "one_for_one",
  children: [
    { id: "counter", start: () => GenServer.start(Counter, { name: "counter" }) }
  ]
})
```

The supervisor automatically re-registers the name when it restarts a crashed child.

---

## Distributed builtins

| JS | Erlang | Purpose |
|----|--------|---------|
| `Node.self()` | `node()` | Current node name |
| `Node.list()` | `nodes()` | List connected nodes |
| `Node.connect("node@host")` | `net_adm:ping('node@host')` | Connect to another node |
| `Process.register("name", pid)` | `erlang:register(name, Pid)` | Register a process by name |
| `Process.whereis("name")` | `erlang:whereis(name)` | Look up a registered process |

---

## How it works

### The eval server (`juice_shell.erl`)

When `juice start` runs, the compiler generates a `juice_shell.erl` module alongside the user's compiled code. This module:

1. Calls `UserModule:main()` synchronously to start the supervision tree
2. Enters a read-eval-print loop using Erlang's built-in `erl_scan`, `erl_parse`, and `erl_eval`
3. Maintains variable bindings across evaluations — if you type `const x = 1`, then `x` is available in subsequent lines
4. Communicates results back to the Rust REPL using a null-byte delimited protocol (to separate eval results from `io:format` output produced by running processes)

The supervision tree stays alive because the shell process (which called `main()`) stays alive in its eval loop. The supervisor is linked to the shell process, so it persists for the entire session.

### The remote eval server (`juice_remote_shell.erl`)

When `juice connect` runs, a second Erlang node starts as a hidden node and connects to the target. Expressions are evaluated on the remote node using `rpc:call(TargetNode, erl_eval, exprs, [Exprs, Bindings])`, so registered names and process state resolve on the remote side.

### Protocol

The Rust REPL sends compiled Erlang expressions (one line per evaluation) to the Erlang VM's stdin. The VM evaluates and writes results to stdout with delimiters:

- Result: `\0JUICE_RESULT\0<value>\0JUICE_END\0`
- Error: `\0JUICE_ERROR\0<reason>\0JUICE_END\0`

Any other stdout output (from `io:format` in GenServer callbacks, OTP reports, etc.) passes through directly to the user's terminal.

---

## Limitations

- **No variable propagation from scripts**: Variables assigned in the `.ts` file (like `sup`) are not available in the REPL. Use named GenServers and call by name instead.
- **One GenServer per file**: The current architecture compiles all callbacks into a single Erlang module. Multiple GenServer types in one file would require a module system.
- **Fixed supervisor flags**: `intensity` (3) and `period` (5 seconds) are hardcoded — not yet configurable from JS.
- **EPMD required for multi-node**: Distributed Erlang needs the Erlang Port Mapper Daemon, which starts automatically with the first named node.
