package com.gladkywdsd.desktopworkspace;

import android.app.Activity;
import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.widget.Button;
import android.widget.TextView;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Shared colours, small widget builders and the saved connection. */
final class Ui {
    static final int BG = Color.parseColor("#11131a");
    static final int CARD = Color.parseColor("#1b1e29");
    static final int TEXT = Color.parseColor("#e6e8ef");
    static final int MUTED = Color.parseColor("#8a90a3");
    static final int ACCENT = Color.parseColor("#7aa2f7");
    static final int DANGER = Color.parseColor("#f7768e");
    static final int OK = Color.parseColor("#9ece6a");

    /** Network calls never run on the UI thread. */
    static final ExecutorService IO = Executors.newCachedThreadPool();

    private static final String PREFS = "bridge";

    static int dp(Context c, int value) {
        return Math.round(value * c.getResources().getDisplayMetrics().density);
    }

    static TextView text(Context c, String s, int sizeSp, int color) {
        TextView t = new TextView(c);
        t.setText(s);
        t.setTextSize(sizeSp);
        t.setTextColor(color);
        return t;
    }

    static TextView title(Context c, String s) {
        TextView t = text(c, s, 22, TEXT);
        t.setTypeface(Typeface.DEFAULT_BOLD);
        return t;
    }

    static Button button(Context c, String label, int color) {
        Button b = new Button(c);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(BG);
        b.setTypeface(Typeface.DEFAULT_BOLD);
        b.setBackground(rounded(color, dp(c, 10)));
        b.setGravity(Gravity.CENTER);
        int pad = dp(c, 12);
        b.setPadding(pad, 0, pad, 0);
        return b;
    }

    static GradientDrawable rounded(int color, int radius) {
        GradientDrawable d = new GradientDrawable();
        d.setColor(color);
        d.setCornerRadius(radius);
        return d;
    }

    static int statusColor(String status) {
        if ("WORKING".equals(status)) return ACCENT;
        if ("WAITING".equals(status) || "NEEDS_INPUT".equals(status)) return Color.parseColor("#e0af68");
        if ("EXITED".equals(status) || "ERROR".equals(status)) return DANGER;
        return OK;
    }

    // ---- saved connection ----

    static SharedPreferences prefs(Context c) {
        return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    static Bridge savedBridge(Context c) {
        Bridge bridge = Bridge.deserialize(prefs(c).getString("bridge", null));
        return bridge != null && bridge.serialize().contains("\"token\"") ? bridge : null;
    }

    static void saveBridge(Context c, Bridge bridge) {
        prefs(c).edit().putString("bridge", bridge == null ? null : bridge.serialize()).apply();
    }

    /** This installation's ID, sent with pairing requests so the PC recognises the phone. */
    static String deviceId(Context c) {
        SharedPreferences p = prefs(c);
        String id = p.getString("deviceId", null);
        if (id == null) {
            id = Bridge.randomHex(16);
            p.edit().putString("deviceId", id).apply();
        }
        return id;
    }

    static void toast(Activity a, String message) {
        a.runOnUiThread(() -> android.widget.Toast.makeText(a, message, android.widget.Toast.LENGTH_LONG).show());
    }

    /** The bridge's error codes in words. */
    static String explain(Exception e) {
        if (e instanceof Bridge.BridgeException) {
            String code = ((Bridge.BridgeException) e).code;
            switch (code) {
                case "not_paired": return "This phone is no longer paired with the PC. Pair it again.";
                case "scan_fresh_desktop_pairing_qr": return "That pairing code was already used or has expired. Create a new one on the PC.";
                case "pairing_blocked": return "The PC rejected this phone earlier. Remove it from the PC's rejected list first.";
                case "pairing_request_already_pending_or_rate_limited": return "A pairing request is already waiting on the PC.";
                case "desktop_not_running": return "Desktop Workspace is not running on the PC.";
                case "harness_not_installed": return "That harness is not installed on the PC.";
                case "invalid_workspace": return "That folder does not exist on the PC.";
                case "no_such_session": return "That session has ended.";
                default: return "The PC refused: " + code;
            }
        }
        String message = e.getMessage() == null ? e.toString() : e.getMessage();
        if (message.contains("certificate_mismatch")) {
            return "The PC's security certificate changed. Pair again with a fresh code.";
        }
        return "Cannot reach the PC: " + message;
    }
}
