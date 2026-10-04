import { copyFile, mkdir, readFile, realpath } from "node:fs/promises";
import { resolve } from "node:path";

const revision = "7e99edab8e32f9f9ea2b15f249ca8d4d67237410";
const expectedHash = "3e8c1a3f17f6d1a2fbad509b4454908d5edb9bf86e8ab8595a029e14418c0ad6";
const sourceArgument = Bun.argv[2];
const destinationArgument = Bun.argv[3];
if (!sourceArgument || !destinationArgument) {
  throw new Error("Provide a pinned SCOWL source checkout and a new content directory");
}
const source = await realpath(sourceArgument);
const destination = resolve(destinationArgument);
async function run(command: string[]): Promise<string> {
  const child = Bun.spawn(command, { cwd: source, stdout: "pipe", stderr: "pipe" });
  const [output, , code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  if (code !== 0) {
    throw new Error("Pinned dictionary preparation failed; check source and build prerequisites");
  }
  return output;
}
if ((await run(["git", "rev-parse", "HEAD"])).trim() !== revision) {
  throw new Error("Dictionary source must match the reviewed release revision");
}
await run(["git", "diff", "--quiet", "HEAD", "--"]);
// Rebuild from checked source rather than trusting an existing SQLite database.
await run(["make", "-B"]);
const recipe = ["./scowl", "word-list", "60", "A", "1", "--wo-poses=abbr", "--categories="];
const raw = await run(recipe);
const words = [
  ...new Set(
    raw
      .split(/\r?\n/)
      .filter((word) => /^[a-z]{2,15}$/.test(word))
      .map((word) => word.toUpperCase()),
  ),
].sort();
const content = `${words.join("\n")}\n`;
const digest = new Bun.CryptoHasher("sha256").update(content).digest("hex");
if (words.length !== 78026 || digest !== expectedHash) {
  throw new Error("Dictionary output differs from the reviewed recipe");
}
const copyright = await readFile(resolve(source, "Copyright"));
// Refuse an existing directory so deployed content and recovery inputs cannot be overwritten.
await mkdir(destination, { mode: 0o700 });
await Bun.write(resolve(destination, "words.txt"), content);
await Bun.write(resolve(destination, "Copyright"), copyright);
await copyFile(resolve(source, "README.md"), resolve(destination, "SOURCE-README.md"));
await Bun.write(
  resolve(destination, "provenance.json"),
  `${JSON.stringify(
    {
      version: 1,
      dictionaryName: "scowl-en-us-60",
      dictionaryRevision: "2026.02.25-recipe1",
      source: "https://github.com/en-wl/wordlist",
      sourceRevision: revision,
      recipe,
      filter:
        "lowercase ASCII words of length 2..15; uppercase; unique; lexical sort; LF trailing newline",
      wordCount: words.length,
      sha256: digest,
      copyrightSha256: new Bun.CryptoHasher("sha256").update(copyright).digest("hex"),
      usage:
        "General American English spelling vocabulary; not an official tournament Scrabble dictionary",
    },
    null,
    2,
  )}\n`,
);
console.log(`Prepared ${words.length} words with verified provenance and retained license notices`);
