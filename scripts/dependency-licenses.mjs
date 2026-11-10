import {readFileSync,writeFileSync} from 'node:fs';
import {resolve,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const metadata=JSON.parse(readFileSync(resolve(root,'target/dependency-metadata.json'),'utf8').replace(/^\uFEFF/,''));
const packages=metadata.packages.filter(p=>p.source).sort((a,b)=>a.name.localeCompare(b.name)||a.version.localeCompare(b.version));
if(packages.some(p=>!p.license))throw new Error('Dependency without a declared license');
writeFileSync(resolve(root,'DEPENDENCY_LICENSES.md'),'# Resolved dependency license inventory\n\nGenerated from `cargo metadata --locked --format-version 1`. Includes target-specific and development dependencies, not only the Windows binary. See THIRD_PARTY_NOTICES.md for fonts and OCR models.\n\n| Package | Version | Declared license |\n|---|---|---|\n'+packages.map(p=>`| ${p.name} | ${p.version} | ${p.license.replaceAll('|','/')} |`).join('\n')+'\n');
console.log(`${packages.length} dependency license declarations recorded.`);
