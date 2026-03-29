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
