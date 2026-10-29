package app.valanium;

/** Labels describe evidence, never optimistic network assumptions. */
final class DeliveryStatus {
    static String label(String state, boolean online, boolean read) {
        if (read) return I18n.t("✓✓ Прочитано");
        if (state == null) return I18n.t("Сохраняется…");
        switch (state) {
            case "queued": return online ? I18n.t("В очереди · ожидает Tor") : I18n.t("В очереди · нет связи");
            case "sending": return I18n.t("Анонимная отправка…");
            case "accepted": return I18n.t("✓ Принято сервером");
            case "delivered": return I18n.t("✓✓ Доставлено");
            case "failed": return I18n.t("Не сохранено · повторите отправку");
            case "expired": return I18n.t("Срок доставки истёк");
            default: return I18n.t("Статус неизвестен");
        }
    }
}
