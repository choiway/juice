const pid = spawn(() => {
  receive((msg) => {
    console.log("got: " + msg)
  })
})

send(pid, "hello from another process!")
