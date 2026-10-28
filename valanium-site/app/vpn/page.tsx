import type { Metadata } from 'next';
import { ServicePage } from '../components/service-page';
import { SERVICES } from '../services';

const service = SERVICES.find((item) => item.id === 'vpn')!;

const TITLE = 'Valanium VPN — выход в сеть через свои узлы';
const DESCRIPTION =
  'Клиент Valanium VPN для Windows готов. Для запуска публичной VPN-сети нужен бюджет на аренду серверов, трафик и обслуживание. Исходный код открыт на GitHub.';

export const metadata: Metadata = {
  title: { absolute: TITLE },
  description: DESCRIPTION,
  alternates: { canonical: 'https://valanium.com/vpn' },
  openGraph: {
    title: TITLE, description: DESCRIPTION, url: 'https://valanium.com/vpn',
    siteName: 'Valanium', locale: 'ru_RU', type: 'website',
    images: [{ url: '/og.png', width: 1200, height: 630, alt: TITLE }],
  },
  twitter: { card: 'summary_large_image', title: TITLE, description: DESCRIPTION, images: ['/og.png'] },
};

export default function VpnPage() {
  return (
    <ServicePage
      service={service}
      page="vpn"
      titleLines={[{ text: 'Выход в сеть' }, { text: 'через свои узлы' }]}
      shot="/media/vpn-phone.png"
      lead="Клиент для Windows готов. Следующий шаг — запуск VPN-сети: для этого нужен бюджет на аренду серверов, сетевой трафик и обслуживание."
      note="Клиент готов · публичная сеть ожидает бюджета на серверы"
      sourceUrl="https://github.com/ifny75/Valanium_Vpn"
      closingTitle="Клиент готов. Нужны серверы."
      closingBody="Разработка клиента завершена. Публичные подключения откроем после финансирования и подготовки серверной инфраструктуры; дату запуска пока не объявляем."
      sections={[
        {
          kicker: 'Готово',
          icon: 'bolt',
          title: 'Клиент для Windows',
          body: 'Приложение на WireGuard готово. Исходный код клиента и серверной части опубликован на GitHub для самостоятельной сборки и развёртывания.',
        },
        {
          kicker: 'Следующий шаг',
          icon: 'no-log',
          title: 'Бюджет на инфраструктуру',
          body: 'Для запуска нужны средства на отдельные VPN-серверы, пропускную способность и обслуживание. Готовый клиент сам по себе не обеспечивает работу публичной сети.',
        },
        {
          kicker: 'Границы',
          icon: 'limit',
          title: 'Чего VPN не делает',
          body: 'VPN меняет того, кто видит ваш трафик, но не убирает наблюдателя совсем: им становимся мы. Полной анонимности он не даёт — для неё в мессенджере есть режим Onion. Обещать большее было бы нечестно.',
        },
      ]}
    />
  );
}
