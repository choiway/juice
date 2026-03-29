for (let i = 0; i < 5; i++) {
  spawn(() => {
    console.log("process " + i + " started")
  })
}
