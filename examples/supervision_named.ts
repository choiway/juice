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
