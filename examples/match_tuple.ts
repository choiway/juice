const route = (method, path) => {
  return match([method, path],
    ["get", "/"], () => "home page",
    ["get", "/users"], () => "list users",
    ["post", "/users"], () => "create user",
    _, () => "not found"
  )
}

console.log(route("get", "/"))
console.log(route("post", "/users"))
console.log(route("get", "/about"))
