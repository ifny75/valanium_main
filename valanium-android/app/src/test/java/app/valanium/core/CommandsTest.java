package app.valanium.core;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

/**
 * Сборка команд — обычная Java, поэтому проверяется без эмулятора:
 * {@code ./gradlew :app:test}.
 *
 * <p>Главное здесь — экранирование: текст сообщения пишет пользователь, и
 * кавычка или перевод строки не должны ломать разбор JSON на стороне ядра.
 */
public class CommandsTest {

    @Test
    public void recoveryUsesOnlyTheSeedPhrase() {
        assertEquals("{\"type\":\"recovery_code\"}", Commands.recoveryCode());
        assertEquals("{\"type\":\"recover\",\"url\":\"wss://x/ws\",\"code\":\"24 words\"}",
                Commands.recover("wss://x/ws", "24 words"));
    }

    @Test
    public void editsKeepTheMessageIdAndEscapeText() {
        assertEquals("{\"type\":\"edit_message\",\"conversation\":\"c\",\"id\":\"id\",\"body\":\"a\\\"b\",\"for_both\":true}",
                Commands.editMessage("c", "id", "a\"b"));
    }

    @Test
    public void simpleCommandsAreLiteral() {
        assertEquals("{\"type\":\"status\"}", Commands.status());
        assertEquals("{\"type\":\"conversations\"}", Commands.conversations());
        assertEquals("{\"type\":\"disconnect\"}", Commands.disconnect());
    }

    @Test
    public void connectCarriesUrl() {
        assertEquals(
                "{\"type\":\"connect\",\"url\":\"wss://valanium.example/ws\"}",
                Commands.connect("wss://valanium.example/ws"));
    }

    @Test
    public void registrationCannotIncludeAPersonalHandle() {
        String expected = "{\"type\":\"register\",\"url\":\"wss://x/ws\",\"handle\":null,\"invite\":\"code\"}";
        assertEquals(expected, Commands.register("wss://x/ws", "code"));
    }

    @Test
    public void historyAndFingerprint() {
        assertEquals(
                "{\"type\":\"history\",\"conversation\":\"aabb\",\"limit\":40,\"before\":null}",
                Commands.history("aabb", 40, null));
        assertEquals("{\"type\":\"fingerprint\",\"identity\":\"ff\"}", Commands.fingerprint("ff"));
    }

    /** Курсор следующей страницы уезжает как есть — ядро его само и выдало. */
    @Test
    public void historyCarriesThePageCursor() {
        assertEquals(
                "{\"type\":\"history\",\"conversation\":\"aabb\",\"limit\":40,\"before\":\"1700:12\"}",
                Commands.history("aabb", 40, "1700:12"));
    }

    /** Документ правил уезжает как есть — его собрало ядро, а не мы. */
    @Test
    public void privacyDocumentTravelsWhole() {
        assertEquals(
                "{\"type\":\"privacy_set\",\"privacy\":{\"voice\":{\"scope\":\"nobody\"}}}",
                Commands.privacySet("{\"voice\":{\"scope\":\"nobody\"}}"));
        assertEquals("{\"type\":\"privacy_get\"}", Commands.privacyGet());
    }

    @Test
    public void directoryCommandsCarryTheDevice() {
        assertEquals(
                "{\"type\":\"directory_set\",\"device\":\"aa\",\"standing\":\"blocked\"}",
                Commands.directorySet("aa", "blocked"));
        assertEquals(
                "{\"type\":\"directory_forget\",\"device\":\"aa\"}",
                Commands.directoryForget("aa"));
    }

    /** Пустая заметка уезжает как null, а не пустой строкой. */
    @Test
    public void inviteWithoutLabelSendsNull() {
        assertEquals(
                "{\"type\":\"pass_invite\",\"label\":null,\"one_time\":true,\"ttl_sec\":3600}",
                Commands.passInvite("", true, 3600));
        assertEquals(
                "{\"type\":\"pass_invite\",\"label\":\"для Миры\",\"one_time\":false,\"ttl_sec\":0}",
                Commands.passInvite("для Миры", false, 0));
    }

    @Test
    public void accessCommandsAreShaped() {
        assertEquals("{\"type\":\"access_get\"}", Commands.accessGet());
        assertEquals("{\"type\":\"access_set\",\"policy\":\"passes\"}", Commands.accessSet("passes"));
        assertEquals("{\"type\":\"pass_revoke\",\"hash\":\"ff\"}", Commands.passRevoke("ff"));
    }

    @Test
    public void quotesInsideMessageAreEscaped() {
        assertEquals(
                "{\"type\":\"send\",\"recipient_device\":\"ab\",\"body\":\"он сказал \\\"да\\\"\"}",
                Commands.send("ab", "он сказал \"да\""));
    }

    @Test
    public void newlinesAndBackslashesAreEscaped() {
        assertEquals(
                "{\"type\":\"send\",\"recipient_device\":\"ab\",\"body\":\"первая\\nвторая\"}",
                Commands.send("ab", "первая\nвторая"));
        assertEquals(
                "{\"type\":\"send\",\"recipient_device\":\"ab\",\"body\":\"C:\\\\путь\"}",
                Commands.send("ab", "C:\\путь"));
    }

    @Test
    public void controlCharactersBecomeEscapes() {
        assertEquals(
                "{\"type\":\"send\",\"recipient_device\":\"ab\",\"body\":\"до\\u0001после\"}",
                Commands.send("ab", "до\u0001после"));
    }

    @Test
    public void nonAsciiStaysAsIs() {
        // JSON — это UTF-8, экранировать кириллицу незачем.
        assertEquals(
                "{\"type\":\"send\",\"recipient_device\":\"ab\",\"body\":\"привет ✓\"}",
                Commands.send("ab", "привет ✓"));
    }
}
