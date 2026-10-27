'use client';
import { useNetworkStatus } from './use-network-status';
import { NodeDetails } from './node-details';
export function LiveNetwork() {
  const { data, failed, busy, refresh } = useNetworkStatus();
  return <section className="network-overview" id="network"><div className="network-overview-inner shell">
    <div className="network-overview-intro"><span className="section-eyebrow"><i /> Инфраструктура Valanium</span>
      <h2>Состояние сети.<br />Сейчас.</h2><p>Нажмите на узел: страна, нагрузка и последняя проверка. Обновляем каждые 15 секунд.</p>
      <div className="network-metric"><strong>{data && !failed ? String(data.nodes.filter(n => n.status === 'operational').length).padStart(2, '0') : '—'}</strong><span>систем<br />доступно</span></div>
      <a className="network-status-link" href="/status">Подробный статус <span>↗</span></a>
    </div>
    <div className="network-index live-network"><header><span>VALANIUM / LIVE</span><button type="button" onClick={() => void refresh()} disabled={busy}>{busy ? 'Проверяем…' : 'Обновить ↻'}</button></header>
      <p className="live-network-message" role="status">{failed ? 'Нет свежих данных. Проверяем связь…' : data ? `Проверено ${new Date(data.checkedAt).toLocaleTimeString('ru-RU')}` : 'Получаем состояние узлов…'}</p>
      <ol>{data?.nodes.map(node => <li key={node.id} data-status={failed ? 'unknown' : node.status}>
        <NodeDetails node={node} failed={failed} />
      </li>)}</ol>
      {!data && <div className="network-empty">{failed ? 'Статус временно недоступен' : 'Проверяем веб-узлы и службы'}</div>}
      <small className="network-live-note">Реальные проверки и агрегированные показатели. Без адресов серверов, портов и статистики пользователей.</small>
    </div>
  </div></section>;
}
