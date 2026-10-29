/** The native side owns URLs, signatures, hashing, cache and atomic installation. */
export function createUpdater({ invoke, version, isOnion, prepareTor, status, button, notify, tr }) {
  let busy = false;
  let ready = false;
  let lastAttempt = 0;
  let lastSuccess = 0;
  async function check(manual = false) {
    if (busy) return;
    if (!manual && ((lastAttempt && Date.now() - lastAttempt < 60000)
      || (lastSuccess && Date.now() - lastSuccess < 6 * 60 * 60 * 1000))) return;
    lastAttempt = Date.now();
    busy = true;
    button.disabled = true;
    status.textContent = tr("Проверяем и скачиваем обновления…");
    try {
      const onion = isOnion();
      if (onion) await prepareTor();
      const result = await invoke("prepare_update", { onion });
      lastSuccess = Date.now();
      ready = result.ready === true;
      button.classList.toggle("hidden", !ready);
      status.textContent = ready
        ? `${tr("Обновление готово")} · ${result.version}`
        : `${tr("Установлена актуальная версия")} · ${await version()}`;
      if (ready || manual) notify(status.textContent);
    } catch {
      // Keep a previous verified download usable across a failed periodic check.
      status.textContent = tr("Не удалось проверить обновления. Повторите позже.");
      if (manual) notify(status.textContent);
    } finally {
      busy = false;
      button.disabled = false;
    }
  }
  button.addEventListener("click", async () => {
    if (!ready || busy) return;
    busy = true;
    button.disabled = true;
    try { await invoke("install_update"); }
    catch { notify(tr("Не удалось установить обновление. Проверьте обновления ещё раз.")); }
    finally { busy = false; button.disabled = false; }
  });
  return { check };
}
