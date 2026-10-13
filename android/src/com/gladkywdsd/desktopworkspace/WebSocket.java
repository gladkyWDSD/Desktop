package com.gladkywdsd.desktopworkspace;

import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;
import java.util.Base64;

import javax.net.ssl.SSLSocket;
import javax.net.ssl.SSLSocketFactory;

/**
 * A minimal RFC 6455 client for the bridge's text streams: masked client
 * frames, fragmented messages, ping/pong and close. The certificate check is
 * the pinned one from {@link Bridge}.
 */
public final class WebSocket {
    public interface Listener {
        void onText(String text);

        /** The stream ended; {@code error} is null for a normal close. */
        void onClosed(IOException error);
    }

    private final SSLSocket socket;
    private final OutputStream out;
    private final SecureRandom random = new SecureRandom();
    private volatile boolean closing;

    private WebSocket(SSLSocket socket) throws IOException {
        this.socket = socket;
        this.out = socket.getOutputStream();
    }

    static WebSocket open(SSLSocketFactory factory, String host, int port, String path, String token,
                          Listener listener) throws IOException {
        java.net.Socket plain = new java.net.Socket();
        plain.connect(new InetSocketAddress(host, port), 8000);
        SSLSocket socket = (SSLSocket) factory.createSocket(plain, host, port, true);
        socket.setSoTimeout(30000);
        socket.startHandshake();
        WebSocket ws = new WebSocket(socket);
        byte[] nonce = new byte[16];
        ws.random.nextBytes(nonce);
        String request = "GET " + path + " HTTP/1.1\r\n"
                + "Host: " + host + ":" + port + "\r\n"
                + "Upgrade: websocket\r\n"
                + "Connection: Upgrade\r\n"
                + "Sec-WebSocket-Key: " + Base64.getEncoder().encodeToString(nonce) + "\r\n"
                + "Sec-WebSocket-Version: 13\r\n"
                + (token != null ? "Authorization: Bearer " + token + "\r\n" : "")
                + "\r\n";
        ws.out.write(request.getBytes(StandardCharsets.UTF_8));
        ws.out.flush();
        DataInputStream in = new DataInputStream(socket.getInputStream());
        String head = readHead(in);
        if (!head.startsWith("HTTP/1.1 101")) {
            socket.close();
            int status = 0;
            try {
                status = Integer.parseInt(head.substring(9, 12));
            } catch (RuntimeException ignored) {
            }
            throw new Bridge.BridgeException(status, status == 401 ? "not_paired" : "stream_refused");
        }
        Thread reader = new Thread(() -> ws.readLoop(in, listener), "ws-reader");
        reader.setDaemon(true);
        reader.start();
        return ws;
    }

    private static String readHead(DataInputStream in) throws IOException {
        ByteArrayOutputStream head = new ByteArrayOutputStream();
        int matched = 0;
        while (matched < 4) {
            int b = in.read();
            if (b < 0) throw new IOException("connection closed during upgrade");
            head.write(b);
            matched = (b == (matched % 2 == 0 ? '\r' : '\n')) ? matched + 1 : (b == '\r' ? 1 : 0);
            if (head.size() > 16384) throw new IOException("upgrade response too large");
        }
        return head.toString("UTF-8");
    }

    private void readLoop(DataInputStream in, Listener listener) {
        ByteArrayOutputStream message = new ByteArrayOutputStream();
        IOException failure = null;
        try {
            while (true) {
                int b0 = in.readUnsignedByte();
                int b1 = in.readUnsignedByte();
                boolean fin = (b0 & 0x80) != 0;
                int opcode = b0 & 0x0f;
                long length = b1 & 0x7f;
                if (length == 126) length = in.readUnsignedShort();
                else if (length == 127) length = in.readLong();
                if (length < 0 || length > 16 * 1024 * 1024) throw new IOException("frame too large");
                byte[] mask = null;
                if ((b1 & 0x80) != 0) {
                    mask = new byte[4];
                    in.readFully(mask);
                }
                byte[] payload = new byte[(int) length];
                in.readFully(payload);
                if (mask != null) for (int i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
                switch (opcode) {
                    case 0x0: // continuation
                    case 0x1: // text
                    case 0x2: // binary (not used by these streams; kept with text)
                        message.write(payload);
                        if (fin) {
                            listener.onText(message.toString("UTF-8"));
                            message.reset();
                        }
                        break;
                    case 0x8:
                        if (!closing) send(0x8, payload);
                        return;
                    case 0x9:
                        send(0xA, payload);
                        break;
                    default:
                        break;
                }
            }
        } catch (IOException e) {
            if (!closing) failure = e;
        } finally {
            try {
                socket.close();
            } catch (IOException ignored) {
            }
            listener.onClosed(failure);
        }
    }

    private synchronized void send(int opcode, byte[] payload) throws IOException {
        ByteArrayOutputStream frame = new ByteArrayOutputStream();
        frame.write(0x80 | opcode);
        if (payload.length < 126) {
            frame.write(0x80 | payload.length);
        } else {
            frame.write(0x80 | 126);
            frame.write(payload.length >> 8);
            frame.write(payload.length & 0xff);
        }
        byte[] mask = new byte[4];
        random.nextBytes(mask);
        frame.write(mask);
        for (int i = 0; i < payload.length; i++) frame.write(payload[i] ^ mask[i % 4]);
        out.write(frame.toByteArray());
        out.flush();
    }

    public void close() {
        closing = true;
        try {
            send(0x8, new byte[] {0x03, (byte) 0xe8});
        } catch (IOException ignored) {
        }
        try {
            socket.close();
        } catch (IOException ignored) {
        }
    }
}
