// Arrow functions work just like JavaScript — same syntax you already know.
// Under the hood, Juice compiles this to an Erlang function on the BEAM VM.
const greet = (name) => {
  console.log(name)
}

// Function calls work as expected — but this runs on Erlang, not Node.js.
greet("hello")
