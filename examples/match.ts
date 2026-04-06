const describe = (status) => {
  return match(status,
    "ok", () => "all good",
    "error", () => "something broke",
    _, (s) => "unknown: " + s
  )
}

console.log(describe("ok"))
console.log(describe("error"))
console.log(describe("hello"))
