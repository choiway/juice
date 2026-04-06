const Counter = {
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

const pid = GenServer.start(Counter)
console.log(GenServer.call(pid, "increment"))
console.log(GenServer.call(pid, "increment"))
console.log(GenServer.call(pid, "increment"))
GenServer.cast(pid, "reset")
console.log(GenServer.call(pid, "get"))
