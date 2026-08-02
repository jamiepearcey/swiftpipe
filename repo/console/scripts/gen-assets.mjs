// Build-time asset generation. swiftpipe ships real MT sample messages and
// YAML schema definitions under repo/examples; the console consumes them as
// typed JSON so there is no runtime file/endpoint dependency for samples or
// the schema browser (Fable spec §6: solve at build time, no endpoint needed).
import { readFileSync, writeFileSync, readdirSync, existsSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { parse as parseYaml } from "yaml";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", ".."); // repo/console/scripts -> repo
const examplesDir = join(repo, "examples");
const schemasDir = join(examplesDir, "schemas");
const outDir = join(here, "..", "src", "generated");
if (!existsSync(outDir)) mkdirSync(outDir, { recursive: true });

// ---- schema catalog -------------------------------------------------------
const schemas = [];
if (existsSync(schemasDir)) {
  for (const file of readdirSync(schemasDir).filter((f) => f.endsWith(".yaml") || f.endsWith(".yml"))) {
    try {
      const doc = parseYaml(readFileSync(join(schemasDir, file), "utf8"));
      for (const m of doc.messages ?? []) {
        schemas.push({
          mt: String(m.message ?? "").toUpperCase(),
          category: m.category ?? "other",
          version: m.version ?? "",
          coverage: m.coverage ?? null,
          sequences: Object.keys(m.sequences ?? {}),
          fields: (m.fields ?? []).map((f) => ({
            path: f.path ?? "",
            tag: f.tag ?? "",
            qualifier: f.qualifier ?? null,
            name: f.name ?? "",
            type: f.type ?? "",
            required: !!f.required,
            entity: f.entity ?? "",
            column: f.column ?? "",
            options: f.options ?? null,
          })),
        });
      }
    } catch (e) {
      console.warn(`gen-assets: skipping schema ${file}: ${e.message}`);
    }
  }
}
schemas.sort((a, b) => a.mt.localeCompare(b.mt));

// ---- sample messages ------------------------------------------------------
const catByMt = new Map(schemas.map((s) => [s.mt, s.category]));
const samples = [];
if (existsSync(examplesDir)) {
  for (const file of readdirSync(examplesDir).filter((f) => f.endsWith(".fin"))) {
    const fin = readFileSync(join(examplesDir, file), "utf8").replace(/\s+$/, "");
    const mtMatch = file.match(/mt(\d{3})/i);
    const mt = mtMatch ? `MT${mtMatch[1]}` : file.replace(/\.fin$/, "").toUpperCase();
    const variant = /break/i.test(file) ? " (recon break)" : "";
    samples.push({
      id: file.replace(/\.fin$/, ""),
      mt,
      label: `${mt}${variant}`,
      category: catByMt.get(mt) ?? "other",
      fin,
    });
  }
}
samples.sort((a, b) => a.id.localeCompare(b.id));

writeFileSync(join(outDir, "schemas.json"), JSON.stringify(schemas));
writeFileSync(join(outDir, "samples.json"), JSON.stringify(samples));
console.log(`gen-assets: ${schemas.length} schemas, ${samples.length} samples → src/generated/`);
