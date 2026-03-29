const me = self()
console.log(me)

const pid = spawn(() => {
  console.log("spawned process running")
})

send(pid, "hello")
console.log("message sent")
