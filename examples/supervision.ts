const Counter = {
  init: () => ({ count: 0 }),
  handleCall: (msg, state) => {
    if (msg === "boom") {
      throw new Error("crash!")
    } else if (msg === "increment") {
      const next = { count: state.count + 1 }
      return { reply: next.count, state: next }
    } else if (msg === "get") {
      return { reply: state.count, state: state }
    }
  }
}

const sup = Supervisor.start({
  strategy: "one_for_one",
  children: [
    { id: "counter", start: () => GenServer.start(Counter) }
  ]
})

const counter = Supervisor.findChild(sup, "counter")
console.log(GenServer.call(counter, "increment"))
console.log(GenServer.call(counter, "increment"))
GenServer.call(counter, "boom")

const counter2 = Supervisor.findChild(sup, "counter")
console.log(GenServer.call(counter2, "increment"))
