'use client';

import { useState } from 'react';

const MODES = [
  {
    id: 'relay',
    number: '01',
    name: 'Relay',
    summary: 'Быстрое автоматическое соединение',
    badge: 'По выбору',
    lead: 'Оптимальный режим на каждый день',
    description: 'Приложение само выбирает доступный узел и держит задержку минимальной.',
    benefits: ['Самое быстрое подключение', 'Автоматическое восстановление связи', 'Минимальный расход батареи'],
  },
  {
    id: 'multihop',
    number: '02',
    name: 'Multihop',
    summary: 'Маршрут через два независимых узла',
    badge: 'В разработке',
    lead: 'Дополнительный маршрут готовится',
    description: 'Сейчас Multi-hop недоступен. Для подключения используйте Onion или явно выберите Relay в приложении.',
    benefits: ['Не выдаём одиночный relay за два узла', 'Состояние узлов доступно на странице сети'],
  },
  {
    id: 'tor',
    number: '03',
    name: 'Onion · Tor',
    summary: 'Максимальная сетевая приватность',
    badge: 'По умолчанию',
    lead: 'Скрывает прямое подключение к Valanium',
    description: 'Соединение уходит в сеть Tor и подходит для случаев, когда приватность важнее скорости.',
    benefits: ['Скрывает исходный IP от узла', 'Обходит сетевые ограничения', 'Изолирует маршрут от обычного трафика'],
  },
] as const;

export function RouteSelector() {
  const [activeId, setActiveId] = useState<(typeof MODES)[number]['id']>('tor');
  const active = MODES.find((mode) => mode.id === activeId) ?? MODES[0];

  return (
    <div className="route-selector">
      <div className="route-choice-list" aria-label="Режим подключения">
        {MODES.map((mode) => {
          const selected = mode.id === activeId;
          return (
            <button
              className={`route-choice${selected ? ' route-choice-active' : ''}`}
              type="button"
              aria-pressed={selected}
              onClick={() => setActiveId(mode.id)}
              key={mode.id}
            >
              <span>{mode.number}</span>
              <span className="route-choice-copy"><b>{mode.name}</b><small>{mode.summary}</small></span>
              <em>{mode.badge}</em>
              <i aria-hidden="true" />
            </button>
          );
        })}
      </div>

      <article className="route-details" aria-live="polite" key={active.id}>
        <div className="route-details-heading">
          <span>{active.number}</span>
          <div><small>Что даёт режим</small><h3>{active.lead}</h3></div>
        </div>
        <p>{active.description}</p>
        <ul>{active.benefits.map((benefit) => <li key={benefit}>{benefit}</li>)}</ul>
      </article>
    </div>
  );
}
