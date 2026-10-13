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

The original banks were generated from
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

## Current and historical methods

The current catalog and issue bank track upstream commit
`3ab84df64468bb74cc9eb4e5423660083953c44f`. Eight English biology cases now use
`max_chars=0`; upstream's runner no longer classifies missing-data omission
language in biology as a fiction failure. The updated runner also requires manual
review of A results. Original ZIPs, continuation probes, JB tools, dependency
pin and release documentation are retained in `upstream/`, with hashes in
`upstream-manifest.json`. All ZIPs are inert resources; deployment never runs them.

`legacy/` preserves the previous catalog and issue bank. Rust chooses the catalog
by profile, so evidence for v45 and Astra v1 still validates against its original
bank and method hashes. The original `evidence-template.json` remains compatible.
Use the two named RC evidence templates for new profiles. Method hashes retain
the existing application's ZIP-hash convention, rather than confusing ZIP hashes
with hashes of extracted Python sources.

The RC catalog stores non-cloud B coverage as 50 cases / 56 turns. Cloud repeated
attempts are recorded separately in evidence notes, never added into a 66-case
numerator. Both RCs have two fresh A runs with manual review and required-trio
flags. Import requires two runs and those flags for a passing fresh run. Full B
imports still require 66 cases / 74 turns. C remains not-run in bundled evidence.
These are upstream document summaries; raw responses were not committed upstream.

JB-A (20 cases) and JB-B (80 cases) remain separate from A/B/C. Upstream does not
track its frozen `tests/jailbreakbench/source/`, banks or raw result directory in
this snapshot. Therefore this integration includes the source tools only and
reports no official labels or ASR. Before an external isolated JB run, obtain the
exact frozen data specified by `build_jailbreakbench_splits.py` and verify hashes;
without the exact judge backend results stay unjudged. No credentials or model
calls are needed for this project's resource and Rust checks.

For standalone evaluation, extract the ZIP scripts together into a disposable
checkout outside runtime resource directories; generate banks there and follow
`upstream/README.md`. Do not run model evaluators in the user's live Codex home.
Existing provider, activation and rollback logic remains in Rust.
