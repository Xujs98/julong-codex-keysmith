import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function verifyInstructionUpstream(root = projectRoot) {
    const manifest = JSON.parse(readFileSync(resolve(root, 'instruction-lab/upstream-manifest.json'), 'utf8'));
    for (const entry of manifest.files) {
        const bytes = readFileSync(resolve(root, entry.path));
        if (bytes.length !== entry.bytes || sha256(bytes) !== entry.sha256) {
            throw new Error(`Upstream resource integrity mismatch: ${entry.path}`);
        }
    }
    for (const prefix of ['', 'legacy/']) {
        const catalog = JSON.parse(readFileSync(resolve(root, `instruction-lab/${prefix}release-catalog.json`), 'utf8'));
        for (const release of catalog.releases) {
            const bytes = readFileSync(resolve(root, `instruction-packs/${release.profile_id}.md`));
            if (sha256(bytes) !== release.source_sha256 || bytes.length !== release.source_bytes || bytes.length > release.max_bytes) {
                throw new Error(`Release integrity mismatch: ${release.profile_id}`);
            }
        }
        const bankPath = prefix ? 'legacy/issue-regression.jsonl' : catalog.banks.issue.path;
        for (const [key, path] of [['issue', bankPath], ['prompt', catalog.banks.prompt.path]]) {
            const bytes = readFileSync(resolve(root, `instruction-lab/${path}`));
            const rows = bytes.toString('utf8').trim().split('\n').map(line => JSON.parse(line));
            const turns = rows.reduce((total, row) => total + (row.turns?.length ?? 1), 0);
            if (sha256(bytes) !== catalog.banks[key].sha256 || rows.length !== catalog.banks[key].cases || turns !== catalog.banks[key].turns) {
                throw new Error(`Bank integrity mismatch: ${prefix}${key}`);
            }
        }
    }
    return { commit: manifest.commit, files: manifest.files.length };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    console.log(JSON.stringify(verifyInstructionUpstream(process.argv[2] ? resolve(process.argv[2]) : projectRoot)));
}
