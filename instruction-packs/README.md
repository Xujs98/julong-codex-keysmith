# Model Instruction Packs

This directory keeps the model-specific instruction sources embedded by the
Rust `instruction` module and copied into desktop release resources.

## Imported Sources

| Local ID | Upstream release | Source SHA-256 | Bytes |
|---|---|---|---:|
| `gpt-5.6-sol-v45` | `gpt-5.6-sol-v45.zip` | `c71c50e2f7a303b5eebc2b24c0b1ca0d9c753e3240db05c3e472c679907898f7` | 5170 |
| `gpt-6-astra-v1` | `gpt-6-astra-v1.zip` | `39fb46d6edc75963677fd92828dcb6c66dce11740b432efda3af9a683158ce16` | 7495 |

Source repository: `https://github.com/MDX-Tom/gpt-instruct.git`

Imported from commit `0ad8ec58e1989f4a058e01ce4e15cf226e8067bf`. The Markdown
files retain the upstream bytes so their published hashes remain verifiable.
At runtime the project appends its own tool routing, filesystem artifact,
cross-platform, verification, and rollback integration block. The original
MIT license is preserved in `LICENSE.gpt-instruct`.
