/**
 * Подписывает APK выпуска ключом Valanium — с родословной ротации.
 *
 * # Зачем отдельный шаг, а не signingConfig в Gradle
 *
 * Сборки до 0.7.0 подписывались **отладочным** ключом: другого просто не было.
 * Android не даёт установить обновление, подписанное не тем ключом, чем
 * установленное приложение, — человеку осталось бы удалить его вместе со всей
 * перепиской.
 *
 * Спасает `apksigner rotate`: в APK кладётся родословная, доказывающая, что
 * старый ключ уполномочил новый. Обновление встаёт поверх, ничего не стирая.
 * Выразить это в Gradle нельзя — он подписал бы одним новым ключом, такой APK
 * установился бы начисто, а поверх старого нет, и заметить это можно было бы
 * только по жалобам.
 *
 * Поэтому Gradle отдаёт неподписанный APK — он не устанавливается вовсе, то
 * есть ошибка видна сразу, — а подпись ставится здесь.
 *
 * # Что нужно
 *
 * В `~/.valanium-release/`: `android-release.jks`, `android-release.pass`,
 * `android-lineage.bin`. Плюс отладочный ключ `~/.android/debug.keystore` —
 * **его выбрасывать нельзя**: на Android до 13 подпись проверяется по нему,
 * так работает ротация v3.1.
 *
 *   node deploy/sign-apk.mjs <входной.apk> <выходной.apk>
 */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

const home = homedir();
const keys = join(home, ".valanium-release");
const KEYSTORE = join(keys, "android-release.jks");
const PASSFILE = join(keys, "android-release.pass");
const LINEAGE = join(keys, "android-lineage.bin");
const DEBUG_KEYSTORE = join(home, ".android", "debug.keystore");

/** Самая свежая build-tools: старые не знают про v3.1 и ротацию. */
function buildTools() {
  const root = join(
    process.env.LOCALAPPDATA ?? join(home, "AppData", "Local"),
    "Android", "Sdk", "build-tools",
  );
  if (!existsSync(root)) throw new Error(`не нашёл build-tools: ${root}`);
  const versions = readdirSync(root).sort((a, b) =>
    a.localeCompare(b, undefined, { numeric: true }));
  const latest = versions.at(-1);
  if (!latest) throw new Error(`build-tools пуст: ${root}`);
  return join(root, latest);
}

function run(command, args, options = {}) {
  return execFileSync(command, args, { encoding: "utf8", ...options });
}

const [input, output] = process.argv.slice(2);
if (!input || !output) {
  process.stderr.write("usage: node deploy/sign-apk.mjs <вход.apk> <выход.apk>\n");
  process.exit(2);
}

for (const [what, path] of [
  ["хранилище ключа", KEYSTORE],
  ["пароль", PASSFILE],
  ["родословная ротации", LINEAGE],
  ["отладочный ключ", DEBUG_KEYSTORE],
  ["входной APK", input],
]) {
  if (!existsSync(path)) {
    process.stderr.write(`нет: ${what} — ${path}\n`);
    process.exit(1);
  }
}

const tools = buildTools();
const zipalign = join(tools, process.platform === "win32" ? "zipalign.exe" : "zipalign");

/*
  Зовём сам jar, а не обёртку `apksigner.bat`.

  Node с 20-й версии отказывается запускать .bat и .cmd без оболочки — это
  защита от подстановки команд через аргументы. Запуск через оболочку вернул бы
  ровно ту дыру, от которой защита: в аргументах здесь путь и пароль. Обёртка
  всё равно только зовёт java, так что зовём её сами.
*/
const APKSIGNER_JAR = join(tools, "lib", "apksigner.jar");
function apksign(args, options = {}) {
  return run("java", ["-jar", APKSIGNER_JAR, ...args], options);
}

// Выравнивание до подписи: apksigner подпись не двигает, а zipalign после неё
// её же и сломает. Порядок здесь не вкусовой.
const aligned = join(tmpdir(), `valanium-aligned-${process.pid}.apk`);
run(zipalign, ["-p", "-f", "4", input, aligned]);

// Пароль уезжает через окружение, а не аргументом: аргументы видны в списке
// процессов любому пользователю машины.
const password = readFileSync(PASSFILE, "utf8").trim();

try {
  apksign([
    "sign",
    "--lineage", LINEAGE,
    // Старый первым: им подписываются v1/v2, по которым проверяют
    // устройства, не знающие про ротацию.
    "--ks", DEBUG_KEYSTORE,
    "--ks-pass", "pass:android",
    "--ks-key-alias", "androiddebugkey",
    "--key-pass", "pass:android",
    "--next-signer",
    "--ks", KEYSTORE,
    "--ks-pass", "env:VALANIUM_KS_PASS",
    "--ks-key-alias", "valanium",
    "--key-pass", "env:VALANIUM_KS_PASS",
    "--out", output,
    aligned,
  ], { env: { ...process.env, VALANIUM_KS_PASS: password } });
} finally {
  rmSync(aligned, { force: true });
}

// Проверяем тем же инструментом, которым проверит устройство. Подпись, которую
// не примет телефон, лучше увидеть здесь, а не в отчёте «не обновляется».
const report = apksign(["verify", "--print-certs", "--verbose", output]);
const signers = [...report.matchAll(/certificate DN: (.+)/g)].map((m) => m[1].trim());
if (signers.length < 2) {
  process.stderr.write(
    `в подписи только один ключ — родословная не приложилась:\n${report}\n`,
  );
  process.exit(1);
}

process.stdout.write(report);
process.stdout.write(`\nподписан: ${output}\n`);
