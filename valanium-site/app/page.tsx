/* eslint-disable @next/next/no-html-link-for-pages */
import type { Metadata } from 'next';
import { DynamicHeader } from './components/dynamic-header';
import { SupportDialog } from './components/support-dialog';
import { SERVICES } from './services';

const TITLE = 'Valanium — приватные сервисы: мессенджер, почта, VPN';
const DESCRIPTION =
  'Valanium — набор приватных сервисов с общей инфраструктурой: мессенджер со сквозным шифрованием, почта и VPN. Открытый код, собственные узлы, вход без номера телефона.';

export const metadata: Metadata = {
  title: { absolute: TITLE },
  description: DESCRIPTION,
  alternates: { canonical: 'https://valanium.com/' },
  openGraph: {
    title: TITLE,
    description: DESCRIPTION,
    url: 'https://valanium.com/',
    siteName: 'Valanium',
    locale: 'ru_RU',
    type: 'website',
    images: [{ url: '/og.png', width: 1200, height: 630, alt: TITLE }],
  },
  twitter: { card: 'summary_large_image', title: TITLE, description: DESCRIPTION, images: ['/og.png'] },
};

const NETWORK_NODES = [
  { id: '01', name: 'Главный узел', role: 'Координация сети', flag: '/flags/germany.svg', country: 'Германия' },
  { id: '02', name: 'VPN сервис', role: 'Защищённый выход', flag: '/flags/netherlands.svg', country: 'Нидерланды' },
  { id: '03', name: 'Relay One', role: 'Защищённый маршрут', flag: '/flags/finland.svg', country: 'Финляндия' },
  { id: '04', name: 'Relay Two', role: 'Защищённый маршрут', flag: '/flags/germany.svg', country: 'Германия' },
  { id: '05', name: 'Relay Four', role: 'Защищённый маршрут', flag: '/flags/sweden.svg', country: 'Швеция' },
] as const;

/*
  Главная — витрина: заголовок, обещание, три значка.

  Первый экран занимает верх страницы целиком. Раньше он лежал островом —
  панель со скруглением и тенью на тёмном фоне; вокруг острова, однако, был
  тот же тёмный фон, и рамка отделяла первый экран от пустоты. Фон полосы
  идёт до краёв окна, текст остаётся в общей колонке, и первый экран получает
  ту же левую границу, что все разделы ниже.

  Кнопок «Войти» и «Зарегистрироваться» здесь нет намеренно. Учётной записи на
  сайте не существует: профиль заводится в приложении, ключи не покидают
  устройства, и сервер их не хранит — заводить вход в веб значило бы обещать
  то, чего нет и по замыслу быть не должно. Поэтому кнопки ведут туда, где
  действие действительно есть: к загрузке и к устройству сети.
*/
export default function Home() {
  return (
    <main id="top">
      <DynamicHeader page="hub" />

      <section className="stage">
        <div className="glass">
          <span className="glass-glow" aria-hidden="true" />
          <span className="stage-signal-field" aria-hidden="true"><i /><i /><i /><i /><i /><i /><i /><i /></span>

          <div className="glass-inner">
            {/*
              Что именно уже работает — на первом экране, а не после прокрутки.
              Из трёх сервисов выпущен один, и человеку это нужно знать до
              того, как он нажмёт «Скачать».
            */}
            <span className="stage-status">
              <i aria-hidden="true" />
              <b>Мессенджер работает</b>
              <em>Windows и Android</em>
            </span>

            <h1><span>Приватность</span> начинается здесь</h1>
            <p>Сквозное шифрование, свои узлы и открытый код. Аккаунт заводится
              в приложении, без номера телефона.</p>

            <div className="stage-actions">
              <a className="stage-button stage-button-primary" href="/messenger#download">Скачать</a>
              <a className="stage-button" href="/messenger#server">Как устроено</a>
            </div>

            <div className="stage-tiles">
              {SERVICES.map((service) => (
                <a
                  key={service.id}
                  className={`stage-tile${service.ready ? '' : ' is-soon'}`}
                  href={service.href}
                >
                  <span className="stage-tile-icon"><img src={service.logo} alt="" /></span>
                  <span className="stage-tile-copy">
                    <b>{service.short}</b>
                    <small>
                      {service.ready ? <i aria-hidden="true" /> : null}
                      {service.ready ? service.badge : 'Скоро'}
                    </small>
                  </span>
                  <i className="stage-tile-arrow" aria-hidden="true" />
                </a>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="network-overview" id="network">
        <div className="network-overview-inner shell">
        <div className="network-overview-intro">
          <span className="section-eyebrow"><i /> Инфраструктура Valanium</span>
          <h2>Пять узлов.<br />Одна сеть.</h2>
          <p>Показываем состояние сервисов, но не раскрываем адреса и внутреннюю топологию.</p>
          <div className="network-metric"><strong>05</strong><span>публичных<br />узлов</span></div>
          <a className="network-status-link" href="/status">Смотреть live-статус <span>↗</span></a>
        </div>

        <div className="network-index" aria-label="Публичные узлы сети Valanium">
          <header><span>VALANIUM / NETWORK</span><b>Доступность · 30 дней</b></header>
          <ol>
            {NETWORK_NODES.map((node, nodeIndex) => (
              <li key={node.id}>
                <span className="network-node-id">{node.id}</span>
                <div className="network-node-name">
                  <span className="network-node-flag"><img src={node.flag} alt={`Флаг: ${node.country}`} /></span>
                  <span><strong>{node.name}</strong><small>{node.country}</small></span>
                </div>
                <p>{node.role}</p>
                <div className="network-signal" aria-label="Узел стабильно доступен последние 30 дней">
                  {Array.from({ length: 18 }, (_, barIndex) => (
                    <i
                      key={barIndex}
                      style={{ animationDelay: `${-(nodeIndex * 0.17 + barIndex * 0.055)}s` }}
                    />
                  ))}
                </div>
                <em>Онлайн</em>
              </li>
            ))}
          </ol>
        </div>
        </div>
      </section>

      <section className="manifesto">
        <div className="manifesto-inner shell">
        <div className="manifesto-kicker"><span>01</span><p>Приватность по умолчанию</p></div>
        <h2>Личное<br />остаётся <em>личным.</em></h2>
        <div className="manifesto-foot">
          <p>Содержание видите только вы и собеседник.</p>
          <span>Valanium передаёт сообщения — и не читает их.</span>
        </div>
        </div>
      </section>

      {/*
        Светлая полоса — та же, что на странице мессенджера. Тёмная страница без
        разрыва читается как одно полотно, и граница между разделами теряется.
        Смена фона обозначает её, не добавляя ни слова текста.
      */}
      <section className="routes" id="shared">
        <div className="routes-inner shell">
          <div className="routes-copy">
            <div className="routes-heading">
              <span>Общая основа</span>
              <h2>Три сервиса.<br />Одни правила.</h2>
              <p>Что верно для одного — верно для всех: инфраструктура, отношение к данным и открытый код общие.</p>
            </div>
            <ul className="hub-points">
              <li>
                <b>Свои узлы</b>
                <span>Инфраструктура своя, не арендованная у платформы. Состояние каждого узла видно публично.</span>
              </li>
              <li>
                <b>Минимум данных</b>
                <span>Сервер не хранит того, что не нужно для доставки. Чего нет в базе — того нельзя ни потерять, ни выдать.</span>
              </li>
              <li>
                <b>Открытый код</b>
                <span>Клиенты, ядро и сервер опубликованы под AGPL-3.0. Проверить обещания можно самому.</span>
              </li>
            </ul>
          </div>
          <div className="routes-visual">
            <span>Valanium Messenger</span>
            <div className="routes-device">
              <img className="routes-phone" src="/media/chat-phone.png" alt="Профиль в мессенджере Valanium" />
              <small className="routes-detail routes-detail-key">Ключи на устройстве</small>
              <small className="routes-detail routes-detail-delivery">Доставлено</small>
            </div>
          </div>
        </div>
      </section>

      <section className="service-rail shell" id="services">
        <div className="service-rail-head">
          <span className="section-eyebrow"><i /> Экосистема Valanium</span>
          <p>Не множество разрозненных приложений. Одна спокойная среда для общения, доступа и почты.</p>
        </div>
        <div className="service-rail-list">
          <a className="rail-item rail-item-ready" href="/messenger">
            <span>01</span><img src="/logos/messenger.svg" alt="" /><div><small>УЖЕ ДОСТУПЕН</small><h2>Messenger</h2></div><p>Сквозное шифрование, сообщества и личные сообщения — без номера телефона.</p><b>Открыть <i>↗</i></b>
          </a>
          <a className="rail-item" href="/mail">
            <span>02</span><img src="/logos/mail.svg" alt="" /><div><small>В РАЗРАБОТКЕ</small><h2>Mail</h2></div><p>Почта с вашим именем, в которой нет места рекламному наблюдению.</p><b>Узнать <i>↗</i></b>
          </a>
          <a className="rail-item" href="/vpn">
            <span>03</span><img src="/logos/vpn.svg" alt="" /><div><small>В РАЗРАБОТКЕ</small><h2>VPN</h2></div><p>Защищённый выход в сеть через инфраструктуру, которую можно проверить.</p><b>Узнать <i>↗</i></b>
          </a>
        </div>
      </section>

      <section className="privacy-statement">
        <div className="privacy-statement-inner">
        <div className="privacy-statement-content shell">
          <div className="privacy-statement-copy">
            <span>02 / Минимум данных</span>
            <h2>Меньше<br /><em>следов.</em></h2>
            <p>Храним только то, без чего сервис не сможет работать. Не больше.</p>
            <ul><li>Без номера телефона</li><li>Без рекламного профиля</li><li>С открытым кодом</li></ul>
            <a href="/privacy">Политика конфиденциальности <b>↗</b></a>
          </div>
          <div className="privacy-statement-visual">
            <span>Маршрут сообщения</span>
            <img src="/media/routing.png" alt="Защищённая инфраструктура Valanium" />
          </div>
        </div>
        </div>
      </section>

      <section className="hub-closing shell">
        <div className="closing">
          <div className="closing-copy">
            <div className="closing-mark"><img src="/logos/brand.svg" alt="" /></div>
            <div>
              <span>Без номера телефона</span>
              <h2>Начните с того,<br />что уже работает.</h2>
              <p>Мессенджер выпущен и открыт. Почта и VPN — на подходе.</p>
            </div>
          </div>
          <div className="closing-actions">
            <div className="actions actions-centered">
              <a className="download-button download-button-light" href="/messenger">
                <img className="platform-svg service-logo" src="/logos/messenger.svg" alt="" />
                <span><small>Открыть</small><b>Мессенджер</b></span>
              </a>
            </div>
            <small>Бесплатно · Открытый код · AGPL-3.0</small>
          </div>
        </div>
      </section>

      <footer className="site-footer shell">
        <span>© 2026 Valanium · AGPL-3.0</span>
        <nav aria-label="Служебные ссылки">
          <a href="/status">Статус сети</a>
          <a href="/privacy">Политика конфиденциальности</a>
          <a href="/terms">Соглашение</a>
          <SupportDialog />
          <a href="https://github.com/ifny75/valanium" target="_blank" rel="noreferrer noopener">GitHub</a>
        </nav>
      </footer>
    </main>
  );
}
