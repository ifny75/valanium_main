import type { Metadata } from 'next';
import { DynamicHeader } from '../components/dynamic-header';
import { SupportDialog } from '../components/support-dialog';
export const metadata: Metadata = {title: {absolute: 'Приложения Valanium'}, description: 'Gallery и Calculator для Android. Открытый код и загрузка APK.'};
const apps = [
 {name:'Gallery', label:'Фото и альбомы', version:'4.2.6 preview', file:'/downloads/Gallery-4.2.6-preview.apk', image:'/apps/gallery.png', source:'https://github.com/ifny75/Gallery', compatibility:'Android 10+ · arm64 · 77 МБ', description:'Все фотографии в одном месте. Альбомы, поиск, редактор и создание копий без метаданных.', tags:['Динамическая тема','Удаление метаданных','Редактор']},
 {name:'Calculator', label:'Всё сходится', version:'Тестовая сборка', file:'/downloads/ExactCalculator-test.apk', image:'/apps/calculator.png', source:'https://github.com/ifny75/platform_packages_apps_ExactCalculator/tree/17', compatibility:'Android 12+ · 1,1 МБ', description:'Точные вычисления, история и научные функции. Спокойный интерфейс с цветами системной темы.', tags:['История','Научные функции','Цвета AOSP']}
];
export default function AppsPage(){return <main className="applications-page" id="top"><DynamicHeader page="apps" />
 <section className="applications-heading shell"><a className="download-back" href="/">← На главную</a><span className="section-eyebrow"><i /> Valanium / Android</span><h1>Приложения.<br />Каждый день.</h1><p>Знакомые задачи, новый интерфейс. Открытый код и прямые загрузки с серверов Valanium.</p></section>
 <section className="application-grid shell" aria-label="Приложения Valanium">{apps.map(app=><article className="application-card" key={app.name}>
  <div className="application-visual"><img src={app.image} alt={`Интерфейс ${app.name}`} width="1400" height="400" /></div>
  <div className="application-copy"><span className="section-eyebrow">{app.label}</span><div className="application-title"><h2>{app.name}</h2><span>{app.version}</span></div><p>{app.description}</p><ul>{app.tags.map(tag=><li key={tag}>{tag}</li>)}</ul><small>{app.compatibility}</small>
  <div className="application-actions"><a className="stage-button stage-button-primary" href={app.file} download>Скачать APK ↓</a><a className="application-source" href={app.source} target="_blank" rel="noopener noreferrer">Исходный код ↗</a></div></div>
 </article>)}</section>
 <section className="applications-note shell"><h2>Под вашим контролем</h2><p>Это предварительные сборки, которые устанавливаются отдельно от системных приложений. Gallery проверяет новые совместимые релизы на GitHub и предлагает скачать обновление. Установку подтверждает Android.</p><p>В Gallery удаление метаданных сохраняет новые PNG-копии, оставляя оригиналы. Исходники и история изменений доступны в репозиториях.</p></section>
 <section className="donation-home shell"><div><h2>Поддержать приложения</h2><p>Можно прислать донат на серверы и дальнейшую разработку Valanium.</p></div><SupportDialog variant="header" /></section>
 <footer className="site-footer shell"><span>© 2026 Valanium</span><nav><a href="/">Главная</a><a href="/status">Состояние сети</a><a href="/privacy">Конфиденциальность</a></nav></footer>
 </main>}
