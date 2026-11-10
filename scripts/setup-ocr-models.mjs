// Download unchanged open OCR weights. No document data is uploaded.
import { mkdir, readFile, writeFile, rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const directory=resolve(root,'models');
await mkdir(directory,{recursive:true});
for (const [name,expected] of [
  ['text-detection.rten','f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca'],
  ['text-recognition.rten','e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e']
]) {
  const target=resolve(directory,name);
  const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
  try { if (digest(await readFile(target))===expected) {console.log(`Verified ${name}`);continue;} } catch {}
  let response=await fetch(`https://ocrs-models.s3-accelerate.amazonaws.com/${name}`);
  if (!response.ok) response=await fetch(`https://github.com/deeplabua/deepocr/releases/download/models-v1/${name}`);
  if (!response.ok) throw new Error(`Model sources unavailable: ${response.status}`);
  const bytes=Buffer.from(await response.arrayBuffer());
  if (digest(bytes)!==expected) throw new Error(`Checksum mismatch: ${name}`);
  await writeFile(`${target}.partial`,bytes);
  await rename(`${target}.partial`,target);
  console.log(`Installed verified ${name}`);
}
