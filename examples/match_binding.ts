const handle = (msg) => {
  return match(msg,
    ["ok", _], (result) => "success: " + result,
    ["error", _], (reason) => "failed: " + reason,
    _, () => "unknown"
  )
}

console.log(handle(["ok", "done"]))
console.log(handle(["error", "timeout"]))
console.log(handle("other"))
