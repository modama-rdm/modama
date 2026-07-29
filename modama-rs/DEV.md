# How to...

## Run tests with coverage

`cargo llvm-cov --html` - then check `target/llvm-cov/html/index.html`

## Run and update visual tests

Check the `justfile`:

- `just visual-tests`
- `just update-goldens`
- `just clean-clutter`
