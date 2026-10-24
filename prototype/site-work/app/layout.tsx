import type { Metadata, Viewport } from 'next';
import { Inter, Unbounded } from 'next/font/google';
import './globals.css';
import { RevealObserver } from './components/reveal-observer';

/**
 * Гарнитуры те же, что в приложении: Unbounded на заголовках, Inter в тексте.
 * Сайт и клиент должны выглядеть одним продуктом, а не двумя разными.
 */
const inter = Inter({
  variable: '--font-inter',
  subsets: ['latin', 'cyrillic'],
  display: 'swap',
});

const unbounded = Unbounded({
  variable: '--font-unbounded',
  subsets: ['latin', 'cyrillic'],
  weight: ['400', '600'],
  display: 'swap',
});

/*
  Поисковая выдача.

  Название продукта совпадает с известным редактором заметок, поэтому по одному
  слову «valanium» сюда не выйти и пытаться незачем. Метаданные написаны под
  запросы, где речь именно о переписке: «valanium messenger», «valanium chat»,
  «обсидиан мессенджер», «приватный мессенджер с шифрованием». Отсюда и
  двуязычные ключевые слова, и подробное описание в JSON-LD.
*/
const TITLE = 'Valanium — приватный мессенджер с E2EE-шифрованием';

/** Карточка для превью ссылок: 1200×630, как просят все, кто их рисует. */
export const OG_IMAGE = {
  url: '/og.png',
  width: 1200,
  height: 630,
  alt: 'Valanium — приватный мессенджер с E2EE-шифрованием',
};
const DESCRIPTION =
  'Valanium (Обсидиан) — приватный мессенджер для Windows и Android. Сквозное шифрование по MLS (RFC 9420), переписка шифруется на устройстве, вход без номера телефона, открытый код под AGPL-3.0.';

export const metadata: Metadata = {
  title: {
    default: TITLE,
    // Страницы канала подставляют своё имя, а хвост остаётся общим.
    template: '%s — Valanium',
  },
  description: DESCRIPTION,
  metadataBase: new URL('https://valanium.com'),
  applicationName: 'Valanium',
  keywords: [
    'valanium messenger',
    'valanium chat',
    'valanium мессенджер',
    'обсидиан мессенджер',
    'приватный мессенджер',
    'мессенджер с шифрованием',
    'end-to-end шифрование',
    'e2ee мессенджер',
    'мессенджер без номера телефона',
    'защищённая переписка',
    'MLS RFC 9420',
    'secure messenger',
    'private messenger',
  ],
  authors: [{ name: 'Valanium' }],
  category: 'communication',
  alternates: { canonical: 'https://valanium.com/' },
  openGraph: {
    title: TITLE,
    description: DESCRIPTION,
    url: 'https://valanium.com/',
    siteName: 'Valanium',
    locale: 'ru_RU',
    type: 'website',
    images: [OG_IMAGE],
  },
  twitter: {
    card: 'summary_large_image',
    title: TITLE,
    description: DESCRIPTION,
    // summary_large_image без картинки — это обещание пустой карточки.
    images: [OG_IMAGE.url],
  },
  robots: {
    index: true,
    follow: true,
    googleBot: { index: true, follow: true, 'max-snippet': -1, 'max-image-preview': 'large' },
  },
};

/*
  Разметка для поисковиков: что это за продукт, на каких платформах и почём.
  Без неё «мессенджер» приходится угадывать из текста страницы.
*/
const STRUCTURED_DATA = {
  '@context': 'https://schema.org',
  '@graph': [
    {
      '@type': 'SoftwareApplication',
      name: 'Valanium',
      alternateName: ['Valanium Messenger', 'Valanium Chat', 'Обсидиан мессенджер'],
      applicationCategory: 'CommunicationApplication',
      applicationSubCategory: 'Messenger',
      operatingSystem: 'Windows 10, Windows 11, Android 8.0+',
      description: DESCRIPTION,
      url: 'https://valanium.com/',
      inLanguage: 'ru',
      license: 'https://www.gnu.org/licenses/agpl-3.0.html',
      offers: { '@type': 'Offer', price: '0', priceCurrency: 'RUB' },
      featureList: [
        'Сквозное шифрование по MLS (RFC 9420)',
        'Вход без номера телефона',
        'Публичные и закрытые каналы',
        'Голосовые сообщения и фотографии',
        'Открытый исходный код под AGPL-3.0',
        'Свой сервер-релей',
      ],
    },
    {
      '@type': 'WebSite',
      name: 'Valanium',
      url: 'https://valanium.com/',
      inLanguage: 'ru',
      publisher: { '@id': 'https://valanium.com/#org' },
    },
    {
      // Чтобы поисковик связал сайт, репозиторий и название в одну сущность —
      // иначе «valanium» так и остаётся для него редактором заметок.
      '@type': 'Organization',
      '@id': 'https://valanium.com/#org',
      name: 'Valanium',
      url: 'https://valanium.com/',
      logo: 'https://valanium.com/og.png',
      sameAs: ['https://github.com/ifny75/valanium'],
    },
  ],
};

/** Цвет строки состояния: в metadata это поле устарело, его место здесь. */
export const viewport: Viewport = {
  themeColor: '#070707',
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="ru">
      <head>
        <script
          type="application/ld+json"
          // Разметка статична и собрана здесь же — постороннего ввода в ней нет.
          dangerouslySetInnerHTML={{ __html: JSON.stringify(STRUCTURED_DATA) }}
        />
      </head>
      <body className={`${inter.variable} ${unbounded.variable}`}><RevealObserver />{children}</body>
    </html>
  );
}
