const pid = spawn(() => {
  console.log("hello from spawned process")
})
console.log("main process continues")
