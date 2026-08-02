# How to...

## Run tests with coverage

`cargo llvm-cov --html` - then check `target/llvm-cov/html/index.html`

## Run and update visual tests

Check the `justfile`:

- `just visual-tests`      # runs visual tests and compares to snapshots
- `just update-snapshots`  # updates snapshots and compresses PNGs via oxipng
- `just clean`             # remove .old.png .new.png .diff.png snapshot files
