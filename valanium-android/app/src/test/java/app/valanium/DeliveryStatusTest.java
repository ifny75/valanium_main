package app.valanium;
import org.junit.Test;
import static org.junit.Assert.*;
public class DeliveryStatusTest {
    @Test public void optimisticMessagesNeverClaimServerAcceptance() {
        assertEquals("Сохраняется…", DeliveryStatus.label(null, true, false));
        assertEquals("В очереди · ожидает Tor", DeliveryStatus.label("queued", true, false));
        assertEquals("В очереди · нет связи", DeliveryStatus.label("queued", false, false));
        assertEquals("✓ Принято сервером", DeliveryStatus.label("accepted", false, false));
        assertEquals("✓✓ Доставлено", DeliveryStatus.label("delivered", false, false));
        assertEquals("✓✓ Прочитано", DeliveryStatus.label("accepted", true, true));
        assertEquals("Статус неизвестен", DeliveryStatus.label("unknown", true, false));
    }
}
