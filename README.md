# owox-harness product repo

This product repo contains the devcontainer and the control repo used to
develop the Rust `owox-harness` target harness.

Keep the root visually small:

```text
.devcontainer/
control/
target/                  # runtime only, ignored by git
owox-harness.code-workspace
README.md
.gitignore
```

`control/` is the control repo. It uses Codex CLI only.

`target/` is a runtime checkout or sandbox for the target repo. Its contents are
ignored by this product repo.

`.owox/` is target harness product data. It must not exist in `control/`.

The target repo must not contain control harness context.

## Local Setup

Clone or copy the target repo into `target/`:

```sh
git clone <owox-harness-url> target
```

If `target/` already exists as an empty directory, cloning into it is fine. Its
contents are ignored by this product repo.

Inside the devcontainer:

```sh
cd /workspace/product/control
bash scripts/check-target-cleanliness.sh
```

Start Codex CLI from `/workspace/product/control`.
