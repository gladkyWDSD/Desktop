package com.gladkywdsd.desktopworkspace;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.security.cert.CertificateException;
import java.security.cert.X509Certificate;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Collections;
import java.util.List;
import java.util.Locale;

import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLSocketFactory;
import javax.net.ssl.TrustManager;
import javax.net.ssl.X509TrustManager;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * The PC's harness bridge: HTTPS with the PC's self-signed certificate pinned
 * by the SHA-256 fingerprint from its pairing QR code, and a bearer token once
 * the PC's owner has approved this phone.
 *
 * <p>A PC can be reached at more than one address: its LAN address at home,
 * its Tailscale address (100.x.y.z or a {@code *.ts.net} name) from anywhere.
 * {@link #alternates} keeps the others; {@link #reachable} moves to whichever
 * answers. The certificate pin is the same for all of them, so an address only
 * ever reaches this PC.
 *
 * No Android classes here, so the same code is exercised on a desktop JVM.
 */
public final class Bridge {
    /** Addresses kept besides the one in use. */
    static final int MAX_ALTERNATES = 6;
    /** A quick reachability check: an address that does not answer this fast is skipped. */
    static final int PROBE_TIMEOUT_MS = 3000;

    public final String host;
    public final int port;
    public final String fingerprint;
    /** Other addresses of the same PC, best first. Never contains {@link #host}. */
    public final List<String> alternates;
    private final String token;
    private final SSLSocketFactory sockets;

    public Bridge(String host, int port, String fingerprint, String token) {
        this(host, port, fingerprint, token, Collections.<String>emptyList());
    }

    public Bridge(String host, int port, String fingerprint, String token, List<String> alternates) {
        this.host = host.trim();
        this.port = port;
        this.fingerprint = fingerprint.toLowerCase(Locale.ROOT);
        this.token = token;
        this.alternates = Collections.unmodifiableList(merge(this.host, alternates, Collections.<String>emptyList()));
        this.sockets = pinned(this.fingerprint);
    }

    public Bridge withToken(String newToken) {
        return new Bridge(host, port, fingerprint, newToken, alternates);
    }

    /** The same PC at another address; the old one becomes the first alternate. */
    public Bridge withHost(String newHost) {
        List<String> others = new ArrayList<>();
        others.add(host);
        others.addAll(alternates);
        return new Bridge(newHost, port, fingerprint, token, others);
    }

    /** The same PC, also known at {@code more} (added after the known ones). */
    public Bridge withAlternates(List<String> more) {
        return new Bridge(host, port, fingerprint, token, merge(host, alternates, more));
    }

    /** {@code first} then {@code then}: trimmed, without blanks, duplicates, loopback or {@code host}. */
    static List<String> merge(String host, List<String> first, List<String> then) {
        List<String> out = new ArrayList<>();
        List<String> all = new ArrayList<>(first);
        all.addAll(then);
        for (String candidate : all) {
            if (candidate == null) continue;
            String h = candidate.trim();
            if (h.isEmpty() || h.equalsIgnoreCase(host) || isLoopback(h)) continue;
            boolean seen = false;
            for (String o : out) seen |= o.equalsIgnoreCase(h);
            if (!seen && out.size() < MAX_ALTERNATES) out.add(h);
        }
        return out;
    }

    static boolean isLoopback(String host) {
        return host.startsWith("127.") || host.equals("::1") || host.equalsIgnoreCase("localhost");
    }

    /** Tailscale's 100.64.0.0/10 and fd7a:115c:a1e0::/48, or a MagicDNS name. */
    public static boolean isTailscale(String host) {
        String h = host.trim().toLowerCase(Locale.ROOT);
        if (h.endsWith(".ts.net") || h.endsWith(".ts.net.")) return true;
        if (h.startsWith("fd7a:115c:a1e0:")) return true;
        String[] parts = h.split("\\.");
        if (parts.length != 4) return false;
        try {
            int a = Integer.parseInt(parts[0]);
            int b = Integer.parseInt(parts[1]);
            return a == 100 && b >= 64 && b <= 127;
        } catch (NumberFormatException e) {
            return false;
        }
    }

    /** "Tailscale" or "LAN": how {@link #host} reaches the PC. */
    public String route() {
        return isTailscale(host) ? "Tailscale" : "LAN";
    }

    /** Whether a failure means the address did not answer, rather than the PC refusing. */
    public static boolean unreachable(Exception e) {
        if (e instanceof BridgeException) return false;
        String message = e.getMessage();
        return !(message != null && message.contains("certificate_mismatch"));
    }

    /** A refused request, with the bridge's own error code when it sent one. */
    public static final class BridgeException extends IOException {
        public final int status;
        public final String code;

        BridgeException(int status, String code) {
            super(code + " (HTTP " + status + ")");
            this.status = status;
            this.code = code;
        }
    }

    /** What the PC's pairing QR code carries. */
    public static final class Invitation {
        public final String host;
        public final int port;
        public final String fingerprint;
        public final String secret;

        Invitation(String host, int port, String fingerprint, String secret) {
            this.host = host;
            this.port = port;
            this.fingerprint = fingerprint;
            this.secret = secret;
        }

        /**
         * Accepts the {@code superdesktop://pair?data=…} link the PC shows (URL-safe
         * base64 of a JSON object), or that JSON itself.
         */
        public static Invitation parse(String text) throws IOException {
            String trimmed = text.trim();
            String json = trimmed;
            int data = trimmed.indexOf("data=");
            if (!trimmed.startsWith("{") && data >= 0) {
                String encoded = trimmed.substring(data + 5);
                int end = encoded.indexOf('&');
                if (end >= 0) encoded = encoded.substring(0, end);
                encoded = URLDecoder.decode(encoded, "UTF-8");
                try {
                    json = new String(Base64.getUrlDecoder().decode(encoded), StandardCharsets.UTF_8);
                } catch (IllegalArgumentException e) {
                    throw new IOException("This pairing link is damaged.");
                }
            }
            try {
                JSONObject o = new JSONObject(json);
                String host = o.getString("host");
                String fingerprint = o.getString("fingerprint");
                String secret = o.getString("secret");
                if (host.isEmpty() || fingerprint.length() != 64 || secret.isEmpty()) {
                    throw new IOException("This pairing link is incomplete.");
                }
                return new Invitation(host, o.getInt("port"), fingerprint, secret);
            } catch (JSONException e) {
                throw new IOException("This is not a pairing link from the PC.");
            }
        }
    }

    public static String randomHex(int bytes) {
        byte[] b = new byte[bytes];
        new SecureRandom().nextBytes(b);
        return hex(b);
    }

    static String hex(byte[] bytes) {
        StringBuilder s = new StringBuilder();
        for (byte b : bytes) s.append(String.format("%02x", b & 0xff));
        return s.toString();
    }

    static SSLSocketFactory pinned(String fingerprint) {
        TrustManager pin = new X509TrustManager() {
            @Override
            public void checkClientTrusted(X509Certificate[] chain, String auth) throws CertificateException {
                throw new CertificateException("client certificates are not used");
            }

            @Override
            public void checkServerTrusted(X509Certificate[] chain, String auth) throws CertificateException {
                if (chain == null || chain.length == 0) throw new CertificateException("no certificate");
                try {
                    String seen = hex(MessageDigest.getInstance("SHA-256").digest(chain[0].getEncoded()));
                    if (!MessageDigest.isEqual(seen.getBytes(StandardCharsets.US_ASCII),
                            fingerprint.getBytes(StandardCharsets.US_ASCII))) {
                        throw new CertificateException("certificate_mismatch");
                    }
                } catch (java.security.NoSuchAlgorithmException e) {
                    throw new CertificateException(e);
                }
            }

            @Override
            public X509Certificate[] getAcceptedIssuers() {
                return new X509Certificate[0];
            }
        };
        try {
            SSLContext context = SSLContext.getInstance("TLS");
            context.init(null, new TrustManager[] {pin}, new SecureRandom());
            return context.getSocketFactory();
        } catch (java.security.GeneralSecurityException e) {
            throw new IllegalStateException(e);
        }
    }

    String hostForUrl() {
        return host.contains(":") ? "[" + host + "]" : host;
    }

    /** One JSON request. Throws {@link BridgeException} for an error status. */
    public JSONObject request(String method, String path, JSONObject body) throws IOException {
        return request(method, path, body, 8000, 20000);
    }

    JSONObject request(String method, String path, JSONObject body, int connectMs, int readMs) throws IOException {
        HttpsURLConnection conn = (HttpsURLConnection) new URL("https://" + hostForUrl() + ":" + port + path).openConnection();
        conn.setSSLSocketFactory(sockets);
        // The certificate is pinned above; its name is never meaningful.
        conn.setHostnameVerifier((name, session) -> true);
        conn.setConnectTimeout(connectMs);
        conn.setReadTimeout(readMs);
        conn.setRequestMethod(method);
        conn.setRequestProperty("Accept", "application/json");
        if (token != null) conn.setRequestProperty("Authorization", "Bearer " + token);
        if (body != null) {
            byte[] bytes = body.toString().getBytes(StandardCharsets.UTF_8);
            conn.setDoOutput(true);
            conn.setRequestProperty("Content-Type", "application/json");
            conn.setFixedLengthStreamingMode(bytes.length);
            try (OutputStream out = conn.getOutputStream()) {
                out.write(bytes);
            }
        }
        int status = conn.getResponseCode();
        InputStream in = status >= 400 ? conn.getErrorStream() : conn.getInputStream();
        String text = in == null ? "" : readAll(in);
        JSONObject json;
        try {
            json = text.isEmpty() ? new JSONObject() : new JSONObject(text);
        } catch (JSONException e) {
            throw new BridgeException(status, "invalid_response");
        }
        if (status >= 400) throw new BridgeException(status, json.optString("error", "http_" + status));
        return json;
    }

    static String readAll(InputStream in) throws IOException {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] buf = new byte[16384];
        int n;
        while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
        in.close();
        return out.toString("UTF-8");
    }

    // ---- addresses ----

    /**
     * {@code GET /api/v1/ping}: the bridge's own description, including the
     * addresses it listens on ({@code lanIp}, {@code tailscaleIp}). Needs no
     * token, and the pinned certificate proves it is this PC.
     */
    public JSONObject ping(int timeoutMs) throws IOException {
        return withToken(null).request("GET", "/api/v1/ping", null, timeoutMs, timeoutMs);
    }

    /** The addresses a ping names, Tailscale first: they work away from home. */
    static List<String> advertised(JSONObject ping) {
        List<String> out = new ArrayList<>();
        String tailscale = ping.optString("tailscaleIp", "");
        if (!tailscale.isEmpty() && !"null".equals(tailscale)) out.add(tailscale);
        JSONArray addresses = ping.optJSONArray("addresses");
        for (int i = 0; addresses != null && i < addresses.length(); i++) {
            JSONObject a = addresses.optJSONObject(i);
            if (a != null && a.optBoolean("connectable") && "tailscale".equals(a.optString("kind"))) {
                out.add(a.optString("address"));
            }
        }
        String lan = ping.optString("lanIp", "");
        if (!lan.isEmpty() && !"null".equals(lan)) out.add(lan);
        for (int i = 0; addresses != null && i < addresses.length(); i++) {
            JSONObject a = addresses.optJSONObject(i);
            if (a != null && a.optBoolean("connectable") && "lan".equals(a.optString("kind"))) {
                out.add(a.optString("address"));
            }
        }
        return out;
    }

    /** This bridge, also knowing every address the PC advertises. Unchanged if the ping fails. */
    public Bridge learnAddresses() {
        try {
            return withAlternates(advertised(ping(PROBE_TIMEOUT_MS)));
        } catch (Exception e) {
            return this;
        }
    }

    /**
     * The first of {@link #host} and {@link #alternates} that answers, as the
     * bridge to use from now on, or {@code null} when none does (the phone is
     * offline, or Tailscale is off on one side).
     */
    public Bridge reachable() {
        List<String> candidates = new ArrayList<>();
        candidates.add(host);
        candidates.addAll(alternates);
        for (String candidate : candidates) {
            Bridge b = candidate.equalsIgnoreCase(host) ? this : withHost(candidate);
            try {
                JSONObject ping = b.ping(PROBE_TIMEOUT_MS);
                return b.withAlternates(advertised(ping));
            } catch (Exception ignored) {
                // Try the next address.
            }
        }
        return null;
    }

    // ---- pairing ----

    /** Ask the PC's owner to approve this phone. Returns {requestId, code}. */
    public JSONObject requestPairing(Invitation invitation, String deviceName, String deviceId) throws IOException {
        JSONObject body = new JSONObject();
        try {
            body.put("secret", invitation.secret);
            body.put("deviceName", deviceName);
            body.put("deviceType", "android");
            body.put("deviceId", deviceId);
        } catch (JSONException e) {
            throw new IOException(e);
        }
        return request("POST", "/api/v1/pair", body);
    }

    /** {status: pending | denied | paired, token} */
    public JSONObject pollPairing(String requestId) throws IOException {
        JSONObject body = new JSONObject();
        try {
            body.put("requestId", requestId);
        } catch (JSONException e) {
            throw new IOException(e);
        }
        return request("POST", "/api/v1/pair/poll", body);
    }

    // ---- sessions ----

    public JSONObject harnesses() throws IOException {
        return request("GET", "/api/v1/harnesses", null);
    }

    public JSONObject harnessTypes() throws IOException {
        return request("GET", "/api/v1/harness-types", null);
    }

    public JSONObject workspaces() throws IOException {
        return request("GET", "/api/v1/workspaces", null);
    }

    /** Start a harness on the PC; {@code workspace} null means the PC's current folder. */
    public JSONObject launch(String agentType, String workspace) throws IOException {
        JSONObject body = new JSONObject();
        try {
            body.put("agentType", agentType);
            if (workspace != null) body.put("workspace", workspace);
        } catch (JSONException e) {
            throw new IOException(e);
        }
        return request("POST", "/api/v1/harnesses", body);
    }

    public void sendKeys(String sessionId, String text, boolean enter) throws IOException {
        JSONObject body = new JSONObject();
        try {
            body.put("text", text);
            body.put("enter", enter);
        } catch (JSONException e) {
            throw new IOException(e);
        }
        request("POST", "/api/v1/harnesses/" + sessionId + "/keys", body);
    }

    public void close(String sessionId) throws IOException {
        request("DELETE", "/api/v1/harnesses/" + sessionId, null);
    }

    /** Live screen frames of one session; see {@link WebSocket}. */
    public WebSocket openStream(String sessionId, WebSocket.Listener listener) throws IOException {
        return WebSocket.open(sockets, host, port, "/api/v1/harnesses/" + sessionId + "/stream", token, listener);
    }

    // ---- stored connection ----

    public String serialize() {
        JSONObject o = new JSONObject();
        try {
            o.put("host", host);
            o.put("port", port);
            o.put("fingerprint", fingerprint);
            o.put("token", token);
            o.put("alternates", new JSONArray(alternates));
        } catch (JSONException e) {
            throw new IllegalStateException(e);
        }
        return o.toString();
    }

    public static Bridge deserialize(String text) {
        if (text == null || text.isEmpty()) return null;
        try {
            JSONObject o = new JSONObject(text);
            List<String> alternates = new ArrayList<>();
            JSONArray a = o.optJSONArray("alternates");
            for (int i = 0; a != null && i < a.length(); i++) alternates.add(a.optString(i));
            return new Bridge(o.getString("host"), o.getInt("port"), o.getString("fingerprint"),
                    o.has("token") && !o.isNull("token") ? o.getString("token") : null, alternates);
        } catch (JSONException e) {
            return null;
        }
    }
}
