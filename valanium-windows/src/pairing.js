/** LAN account linking UI. QR secrets live only in this foreground session. */
export function createPairing({ send, invoke, tr, hasIdentity }) {
  let active = false, session = '', qr = '', svg = '', offered = false, modal = null, receiving = false;
  const enabled = () => localStorage.getItem('valanium.pairing.discovery') !== 'false';
  const listen = () => { receiving = true; active = true; void send({ type: 'pairing_listen' }); };
  const stop = () => {
    const wasActive = active || session;
    active = false; session = qr = svg = ''; receiving = false;
    if (wasActive) { void send({ type: 'pairing_discover', enabled: false }); void send({ type: 'pairing_cancel' }); }
    modal?.remove(); modal = null; document.getElementById('pairing-offer')?.remove();
  };
  function start(identity) {
    if (active || !enabled() || document.hidden || !document.hasFocus()) return;
    active = true;
    if (identity) void send({ type: 'pairing_discover', enabled: true }); else listen();
  }
  const button = (text, action) => { const b = document.createElement('button'); b.type = 'button'; b.className = 'ghost-button'; b.textContent = tr(text); b.addEventListener('click', action); return b; };
  const field = (tag, text) => { const el = document.createElement(tag); if (text) el.textContent = tr(text); return el; };
  function update(event) {
    if (!modal) return;
    const state = modal.querySelector('[data-pairing-state]');
    const image = modal.querySelector('img');
    if (event.state === 'listening') {
      image.src = `data:image/svg+xml;base64,${btoa(svg)}`; image.hidden = false;
      state.textContent = tr('Отсканируйте этот QR на устройстве с аккаунтом. QR действует 3 минуты.');
      modal.querySelector('[data-pairing-code]').value = qr;
    } else if (event.state === 'transferring') {
      image.hidden=true; modal.querySelector('[data-pairing-confirm]').hidden=true;
      state.textContent=tr('Переносим чаты и сообщения… ')+event.code;
    } else if (event.state === 'verify') {
      image.hidden = true;
      modal.querySelector('[data-pairing-code]').value = '';
      state.textContent = tr('Сверьте код: ') + event.code;
      const confirm = modal.querySelector('[data-pairing-confirm]'); confirm.hidden = false; confirm.disabled = false;
    } else if (event.state === 'failed') {
      image.hidden = true; image.removeAttribute('src'); modal.querySelector('[data-pairing-code]').value = '';
      modal.querySelector('[data-pairing-confirm]').hidden = true;
      state.textContent = tr('Сопряжение не удалось. Проверьте Wi-Fi и создайте новый QR.');
      qr = svg = '';
    }
  }
  function open() {
    if (modal) return;
    receiving = !hasIdentity();
    modal = field('div'); modal.className = 'modal'; modal.id = 'pairing-modal'; modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true');
    const card = field('div'); card.className = 'modal-card pairing-card';
    card.append(field('h2', 'Подключение аккаунта'), field('p', 'Оба устройства должны быть в одной Wi-Fi сети. Переносятся чаты, сообщения и вложения. Телефон продолжит работать. Подтвердите одинаковый код на обоих экранах.'));
    card.append(field('p', 'При запросе Windows разрешите доступ к частной сети. Гостевая Wi-Fi сеть может блокировать связь между устройствами.'));
    const state = field('p', 'Ожидание сопряжения…'); state.dataset.pairingState = ''; state.setAttribute('aria-live', 'polite'); card.append(state);
    const image = field('img'); image.alt = tr('QR подключения устройства'); image.hidden = true; card.append(image);
    const code = field('textarea'); code.dataset.pairingCode = ''; code.spellcheck = false;
    code.setAttribute('aria-label', tr('Код сопряжения с другого устройства')); code.placeholder = tr('Код сопряжения с другого устройства'); code.readOnly = receiving; card.append(code);
    if (receiving) card.append(button('Создать новый QR', listen));
    else card.append(button('Подключить по коду', () => { const value = code.value.trim(); code.value = ''; active = true; void send({ type: 'pairing_connect', qr: value }); }));
    const password = field('input'); password.type = 'password'; password.autocomplete = 'current-password'; password.placeholder = tr('Пароль приложения, если он установлен');
    password.setAttribute('aria-label', password.placeholder); password.hidden = receiving; card.append(password);
    const confirm = button('Код совпадает — подключить', async () => {
      confirm.disabled = true;
      try {
        if (receiving) await send({ type: 'pairing_confirm', session });
        else await invoke('pairing_confirm', { session, password: password.value || null });
        password.value = ''; state.textContent = tr('Ожидаем подтверждение второго устройства…');
      } catch { confirm.disabled = false; password.value = ''; state.textContent = tr('Не удалось подтвердить. Проверьте пароль приложения.'); }
    });
    confirm.dataset.pairingConfirm = ''; confirm.hidden = true; card.append(confirm);
    card.append(button('Закрыть', stop)); modal.append(card); document.body.append(modal);
    if (receiving) { if (qr) update({ state: 'listening' }); else listen(); }
    card.querySelector('button')?.focus();
  }
  function event(event) {
    if (!active) return;
    if (event.type === 'pairing_found') {
      if (!hasIdentity() || offered || !enabled() || document.hidden || !document.hasFocus() || modal) return;
      offered = true;
      const offer = field('div'); offer.className = 'pairing-offer'; offer.id = 'pairing-offer'; offer.setAttribute('role', 'status');
      offer.append(field('b', 'Рядом найдено устройство Valanium'), field('p', 'Подключить к нему аккаунт? Понадобится отсканировать QR на втором устройстве и сверить код.'), button('Подключить', () => { offer.remove(); open(); }), button('Не сейчас', () => offer.remove()));
      document.body.append(offer); return;
    }
    if (event.state === 'listening' || event.state === 'connecting') {
      session = event.session; active = true;
      if (event.qr) { qr = event.qr; svg = event.qr_svg; }
      update(event); return;
    }
    if (event.session !== session) return;
    if (event.state === 'transferring') update(event);
    else if (event.state === 'verify') { open(); update(event); }
    else if (event.state === 'failed') update(event);
    else if (event.state === 'complete') {
      modal?.remove(); modal = null; session = qr = svg = ''; active = false;
      const result = field('div'); result.className = 'pairing-offer'; result.textContent = tr('Устройство подключено. Чаты и история перенесены.'); document.body.append(result); setTimeout(() => result.remove(), 8000);
      start(hasIdentity());
    }
  }
  for (const id of ['pairing-settings', 'pairing-entry']) document.getElementById(id).addEventListener('click', open);
  for (const toggle of document.querySelectorAll('[data-pairing-discovery]')) {
    toggle.checked = enabled(); toggle.addEventListener('change', () => {
      localStorage.setItem('valanium.pairing.discovery', String(toggle.checked));
      document.querySelectorAll('[data-pairing-discovery]').forEach(other => { other.checked = toggle.checked; });
      if (toggle.checked) start(hasIdentity()); else stop();
    });
  }
  document.addEventListener('visibilitychange', () => {
    document.getElementById('pairing-offer')?.remove();
    if (document.hidden) stop(); else { offered = false; start(hasIdentity()); }
  });
  document.addEventListener('keydown', event => { if (event.key === 'Escape' && modal) { event.stopImmediatePropagation(); stop(); } }, true);
  window.addEventListener('beforeunload', stop);
  return { event, start, stop, failed: () => update({ state: "failed" }) };
}
