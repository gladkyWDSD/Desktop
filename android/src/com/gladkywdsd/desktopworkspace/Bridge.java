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
import java.util.Base64;

import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLContext;
import javax.net.ssl.SSLSocketFactory;
import javax.net.ssl.TrustManager;
import javax.net.ssl.X509TrustManager;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * The PC's harness bridge: HTTPS with the PC's self-signed certificate pinned
 * by the SHA-256 fingerprint from its pairing QR code, and a bearer token once
 * the PC's owner has approved this phone.
 *
 * No Android classes here, so the same code is exercised on a desktop JVM.
 */
public final class Bridge {
    public final String host;
    public final int port;
    public final String fingerprint;
    private final String token;
    private final SSLSocketFactory sockets;

    public Bridge(String host, int port, String fingerprint, String token) {
        this.host = host;
        this.port = port;
        this.fingerprint = fingerprint.toLowerCase();
        this.token = token;
        this.sockets = pinned(this.fingerprint);
    }

    public Bridge withToken(String newToken) {
        return new Bridge(host, port, fingerprint, newToken);
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
        HttpsURLConnection conn = (HttpsURLConnection) new URL("https://" + hostForUrl() + ":" + port + path).openConnection();
        conn.setSSLSocketFactory(sockets);
        // The certificate is pinned above; its name is never meaningful.
        conn.setHostnameVerifier((name, session) -> true);
        conn.setConnectTimeout(8000);
        conn.setReadTimeout(20000);
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
        } catch (JSONException e) {
            throw new IllegalStateException(e);
        }
        return o.toString();
    }

    public static Bridge deserialize(String text) {
        if (text == null || text.isEmpty()) return null;
        try {
            JSONObject o = new JSONObject(text);
            return new Bridge(o.getString("host"), o.getInt("port"), o.getString("fingerprint"), o.optString("token", null));
        } catch (JSONException e) {
            return null;
        }
    }
}
