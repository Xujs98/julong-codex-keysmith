# Instruction Lab

This directory joins the upstream prompt-development gates with the project's
Rust/Tauri production runtime.

## Pipeline

1. Record user feedback and failed cases in the local Codex home.
2. Run A user-feedback checks, B issue regression, and C bilingual medium bank.
3. Import model-run evidence with matching model, reasoning, prompt hash, bank
   hashes, and method identity.
4. Keep production deployment behind source-integrity and release-status checks.
5. Deploy approved or explicitly released snapshots through the existing Rust
   transaction, MITM proxy, monitoring, and rollback path.

The bundled banks were generated from
MDX-Tom/gpt-instruct@0ad8ec58e1989f4a058e01ce4e15cf226e8067bf.
The source generator ZIP hashes are:

- prompt bank: 693521bda2e90d36c5c93fe60de8fb127d3f055c2e8d43ed0fdc38bcdd1206b2
- issue bank: 637fb3ec831aa2979178a5c06a8bcae8101db7472ae204b6adee821d32cc6120
- prompt runner: 2e64cc047299e11363b1ea9678fb1b8c411c3ea288947299a7d106b16dc7dba2
- issue runner: e3666401b1bf98cb205c34761906321bf537826ee2638dcfb4e3a614b3f3fda9
- issue scorer: bcf5b3b66ff92663f7b1e8d5c1d0337ab48cacf8652b94a70bcba7f815dfaee3

Generated bank hashes, gate thresholds, and historical release evidence live in
release-catalog.json. The imported upstream results remain explicit: a formal
release can be production-deployable while its current hard gate is incomplete.
The UI and CLI therefore display both states rather than converting a release
decision into an invented passing score.

Runtime feedback and imported evidence are written below
CODEX_HOME/instruction-lab/; bundled resources remain read-only.

The upstream-generated data and accompanying source material use the MIT license
preserved in LICENSE.gpt-instruct.
