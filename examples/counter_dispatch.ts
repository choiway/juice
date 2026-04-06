const Counter = {
  init: () => ({ count: 0 }),
  handleCall: {
    increment: (state) => {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    },
    get: (state) => ({ reply: state.count, state: state })
  },
  handleCast: {
    reset: (state) => ({ state: { count: 0 } })
  }
}

const pid = GenServer.start(Counter)
console.log(GenServer.call(pid, "increment"))
console.log(GenServer.call(pid, "increment"))
console.log(GenServer.call(pid, "increment"))
GenServer.cast(pid, "reset")
console.log(GenServer.call(pid, "get"))
