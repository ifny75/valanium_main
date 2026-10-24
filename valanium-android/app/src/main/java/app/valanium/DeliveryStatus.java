package app.valanium;

/** Labels describe evidence, never optimistic network assumptions. */
final class DeliveryStatus {
    static String label(String state, boolean online, boolean read) {
        if (read) return "✓✓ Прочитано";
        if (state == null) return "Сохраняется…";
        switch (state) {
            case "queued": return online ? "В очереди · ожидает Tor" : "В очереди · нет связи";
            case "sending": return "Анонимная отправка…";
            case "accepted": return "✓ Принято сервером";
            case "delivered": return "✓✓ Доставлено";
            case "failed": return "Не сохранено · повторите отправку";
            case "expired": return "Срок доставки истёк";
            default: return "Статус неизвестен";
        }
    }
}
