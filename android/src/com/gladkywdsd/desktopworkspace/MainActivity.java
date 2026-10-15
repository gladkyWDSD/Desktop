package com.gladkywdsd.desktopworkspace;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;

/**
 * Pairing with a PC, then its running harness sessions: open one, or start a
 * new harness on the PC.
 */
public final class MainActivity extends Activity {
    private static final long REFRESH_MS = 2000;

    private final Handler main = new Handler(Looper.getMainLooper());
    private Bridge bridge;
    private LinearLayout list;
    private TextView status;
    private boolean resumed;
    private boolean pairing;
    /** A session list request is out: the 2 s refresh must not stack them while an address times out. */
    private boolean loading;
    /** Addresses were learned from the PC since the app started. */
    private boolean learned;

    private final Runnable refresh = new Runnable() {
        @Override
        public void run() {
            if (!resumed || bridge == null) return;
            loadSessions();
            main.postDelayed(this, REFRESH_MS);
        }
    };

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        getWindow().getDecorView().setBackgroundColor(Ui.BG);
        bridge = Ui.savedBridge(this);
        if (!handleLink(getIntent())) showHome();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleLink(intent);
    }

    @Override
    protected void onResume() {
        super.onResume();
        resumed = true;
        main.removeCallbacks(refresh);
        main.post(refresh);
    }

    @Override
    protected void onPause() {
        super.onPause();
        resumed = false;
        main.removeCallbacks(refresh);
    }

    /** A {@code superdesktop://pair} link opened from the PC's QR code. */
    private boolean handleLink(Intent intent) {
        Uri data = intent == null ? null : intent.getData();
        if (data == null || !"pair".equals(data.getHost())) return false;
        startPairing(data.toString(), null);
        return true;
    }

    private void showHome() {
        if (bridge == null) showPairing(null);
        else showSessions();
    }

    private LinearLayout page() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = Ui.dp(this, 18);
        root.setPadding(pad, pad + Ui.dp(this, 16), pad, pad);
        return root;
    }

    // ---- pairing ----

    private void showPairing(String message) {
        pairing = false;
        LinearLayout root = page();
        root.addView(Ui.title(this, "Pair with your PC"));
        TextView how = Ui.text(this,
                "On the PC, open Desktop Workspace → ⚙ Settings → Connections → Add a device → Pair a phone.\n\n"
                        + "Scan the QR code with your camera and open the link with this app, "
                        + "or copy the link on the PC and paste it here.", 15, Ui.MUTED);
        how.setPadding(0, Ui.dp(this, 10), 0, Ui.dp(this, 14));
        root.addView(how);

        EditText link = new EditText(this);
        link.setHint("superdesktop://pair?data=…");
        link.setHintTextColor(Ui.MUTED);
        link.setTextColor(Ui.TEXT);
        link.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        root.addView(link, new LinearLayout.LayoutParams(-1, -2));

        TextView away = Ui.text(this,
                "Not on the PC's network? Connect both devices to Tailscale and enter the PC's Tailscale "
                        + "address (100.x.y.z or name.ts.net), shown on the PC in Connections.", 13, Ui.MUTED);
        away.setPadding(0, Ui.dp(this, 14), 0, Ui.dp(this, 4));
        root.addView(away);
        EditText address = new EditText(this);
        address.setHint("PC address (optional)");
        address.setHintTextColor(Ui.MUTED);
        address.setTextColor(Ui.TEXT);
        address.setSingleLine(true);
        address.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        root.addView(address, new LinearLayout.LayoutParams(-1, -2));

        LinearLayout buttons = new LinearLayout(this);
        buttons.setPadding(0, Ui.dp(this, 10), 0, 0);
        View paste = Ui.button(this, "Paste", Ui.CARD);
        ((TextView) paste).setTextColor(Ui.TEXT);
        paste.setOnClickListener(v -> {
            ClipboardManager clip = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
            ClipData data = clip == null ? null : clip.getPrimaryClip();
            if (data != null && data.getItemCount() > 0 && data.getItemAt(0).getText() != null) {
                link.setText(data.getItemAt(0).getText());
            }
        });
        View pair = Ui.button(this, "Pair", Ui.ACCENT);
        pair.setOnClickListener(v -> startPairing(link.getText().toString(), address.getText().toString()));
        LinearLayout.LayoutParams half = new LinearLayout.LayoutParams(0, Ui.dp(this, 48), 1);
        half.setMargins(0, 0, Ui.dp(this, 8), 0);
        buttons.addView(paste, half);
        buttons.addView(pair, new LinearLayout.LayoutParams(0, Ui.dp(this, 48), 1));
        root.addView(buttons);

        status = Ui.text(this, message == null ? "" : message, 15, Ui.DANGER);
        status.setPadding(0, Ui.dp(this, 16), 0, 0);
        root.addView(status);
        setContentView(root);
    }

    /**
     * {@code address}, when given, is where the phone reaches the PC (its
     * Tailscale address, typically); the address in the link is kept as an
     * alternate for when the phone is back on the PC's network.
     */
    private void startPairing(String linkText, String address) {
        final Bridge.Invitation invitation;
        try {
            invitation = Bridge.Invitation.parse(linkText);
        } catch (Exception e) {
            showPairing(e.getMessage());
            return;
        }
        pairing = true;
        LinearLayout root = page();
        root.addView(Ui.title(this, "Waiting for the PC"));
        TextView code = Ui.text(this, "", 40, Ui.ACCENT);
        code.setGravity(Gravity.CENTER);
        code.setPadding(0, Ui.dp(this, 30), 0, Ui.dp(this, 10));
        String override = address == null ? "" : address.trim();
        Bridge unpaired = new Bridge(invitation.host, invitation.port, invitation.fingerprint, null);
        if (!override.isEmpty() && !override.equalsIgnoreCase(invitation.host)) unpaired = unpaired.withHost(override);
        final Bridge target = unpaired;
        TextView info = Ui.text(this, "Connecting to " + target.host + "…", 15, Ui.MUTED);
        info.setGravity(Gravity.CENTER);
        root.addView(code);
        root.addView(info);
        View cancel = Ui.button(this, "Cancel", Ui.CARD);
        ((TextView) cancel).setTextColor(Ui.TEXT);
        cancel.setOnClickListener(v -> showPairing(null));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, Ui.dp(this, 48));
        lp.setMargins(0, Ui.dp(this, 30), 0, 0);
        root.addView(cancel, lp);
        setContentView(root);

        String name = Build.MANUFACTURER + " " + Build.MODEL;
        Ui.IO.execute(() -> {
            try {
                // The address in the link may be unreachable from here (a LAN
                // address while away): try every address the phone knows.
                Bridge reached = target.reachable();
                Bridge unpairedHere = reached == null ? target : reached;
                JSONObject request = unpairedHere.requestPairing(invitation, name, Ui.deviceId(this));
                String requestId = request.getString("requestId");
                runOnUiThread(() -> {
                    code.setText(request.optString("code"));
                    info.setText("Check that the PC shows this code, then approve this phone there.");
                });
                long until = System.currentTimeMillis() + request.optLong("expiresIn", 120) * 1000;
                while (pairing && System.currentTimeMillis() < until) {
                    Thread.sleep(1500);
                    JSONObject poll = unpairedHere.pollPairing(requestId);
                    String state = poll.optString("status");
                    if ("paired".equals(state)) {
                        Bridge paired = unpairedHere.withToken(poll.getString("token")).learnAddresses();
                        runOnUiThread(() -> {
                            pairing = false;
                            bridge = paired;
                            Ui.saveBridge(this, paired);
                            showSessions();
                            main.removeCallbacks(refresh);
                            main.post(refresh);
                        });
                        return;
                    }
                    if ("denied".equals(state)) {
                        runOnUiThread(() -> showPairing("The PC rejected this phone."));
                        return;
                    }
                }
                if (pairing) runOnUiThread(() -> showPairing("Nobody approved the request in time. Create a new code on the PC."));
            } catch (Exception e) {
                if (pairing) runOnUiThread(() -> showPairing(Ui.explain(e)));
            }
        });
    }

    // ---- sessions ----

    private void showSessions() {
        LinearLayout root = page();
        LinearLayout header = new LinearLayout(this);
        header.setGravity(Gravity.CENTER_VERTICAL);
        TextView title = Ui.title(this, "Sessions");
        header.addView(title, new LinearLayout.LayoutParams(0, -2, 1));
        View menu = Ui.button(this, "⋯", Ui.CARD);
        ((TextView) menu).setTextColor(Ui.TEXT);
        menu.setOnClickListener(v -> showMenu());
        header.addView(menu, new LinearLayout.LayoutParams(Ui.dp(this, 48), Ui.dp(this, 44)));
        root.addView(header);

        status = Ui.text(this, bridge.host, 14, Ui.MUTED);
        status.setPadding(0, Ui.dp(this, 4), 0, Ui.dp(this, 12));
        root.addView(status);

        View launch = Ui.button(this, "+ New session on the PC", Ui.ACCENT);
        launch.setOnClickListener(v -> chooseHarness());
        root.addView(launch, new LinearLayout.LayoutParams(-1, Ui.dp(this, 50)));

        ScrollView scroll = new ScrollView(this);
        list = new LinearLayout(this);
        list.setOrientation(LinearLayout.VERTICAL);
        list.setPadding(0, Ui.dp(this, 12), 0, 0);
        scroll.addView(list);
        root.addView(scroll, new LinearLayout.LayoutParams(-1, 0, 1));
        setContentView(root);
    }

    private void showMenu() {
        new AlertDialog.Builder(this)
                .setTitle(bridge.host + ":" + bridge.port + " · " + bridge.route())
                .setItems(new String[] {"PC address…", "Unpair this phone"}, (d, which) -> {
                    if (which == 0) {
                        chooseAddress();
                        return;
                    }
                    new AlertDialog.Builder(this)
                            .setMessage("Forget this PC? You will need a new pairing code to connect again.")
                            .setPositiveButton("Unpair", (d2, w2) -> {
                                bridge = null;
                                Ui.saveBridge(this, null);
                                showPairing(null);
                            })
                            .setNegativeButton("Cancel", null)
                            .show();
                })
                .show();
    }

    /** Pick one of the PC's known addresses, or enter one (its Tailscale address, say). */
    private void chooseAddress() {
        List<String> known = new ArrayList<>();
        known.add(bridge.host);
        known.addAll(bridge.alternates);
        List<String> labels = new ArrayList<>();
        for (String h : known) {
            String kind = Bridge.isTailscale(h) ? "Tailscale" : "LAN";
            labels.add(h + " · " + kind + (h.equals(bridge.host) ? " · in use" : ""));
        }
        labels.add("Enter an address…");
        new AlertDialog.Builder(this)
                .setTitle("PC address")
                .setItems(labels.toArray(new String[0]), (d, which) -> {
                    if (which < known.size()) useAddress(known.get(which));
                    else enterAddress();
                })
                .show();
    }

    private void enterAddress() {
        EditText field = new EditText(this);
        field.setHint("100.x.y.z or name.ts.net");
        field.setSingleLine(true);
        field.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        int pad = Ui.dp(this, 20);
        LinearLayout box = new LinearLayout(this);
        box.setPadding(pad, Ui.dp(this, 8), pad, 0);
        box.addView(field, new LinearLayout.LayoutParams(-1, -2));
        new AlertDialog.Builder(this)
                .setTitle("PC address")
                .setMessage("The PC's Tailscale address is in Desktop Workspace → ⚙ Settings → Connections on the PC.")
                .setView(box)
                .setPositiveButton("Connect", (d, w) -> {
                    String h = field.getText().toString().trim();
                    if (!h.isEmpty()) useAddress(h);
                })
                .setNegativeButton("Cancel", null)
                .show();
    }

    /** Switch to {@code host} once the PC answers there; keep the current address if not. */
    private void useAddress(String host) {
        Bridge current = bridge;
        Bridge candidate = host.equalsIgnoreCase(current.host) ? current : current.withHost(host);
        Ui.toast(this, "Connecting to " + host + "…");
        Ui.IO.execute(() -> {
            try {
                Bridge answered = candidate.withAlternates(Bridge.advertised(candidate.ping(Bridge.PROBE_TIMEOUT_MS * 2)));
                runOnUiThread(() -> {
                    if (current != bridge) return;
                    bridge = answered;
                    Ui.saveBridge(this, answered);
                    if (status != null) status.setText(answered.host + " · connected via " + answered.route());
                    loadSessions();
                });
            } catch (Exception e) {
                Ui.toast(this, "The PC did not answer at " + host + ". " + Ui.explain(e));
            }
        });
    }

    private void loadSessions() {
        if (loading || bridge == null) return;
        loading = true;
        Bridge current = bridge;
        boolean learn = !learned;
        Ui.IO.execute(() -> {
            Bridge using = current;
            try {
                JSONArray sessions;
                try {
                    sessions = using.harnesses().optJSONArray("harnesses");
                } catch (Exception e) {
                    // The address in use stopped answering (left home Wi-Fi,
                    // Tailscale turned off): try the PC's other addresses.
                    if (!Bridge.unreachable(e)) throw e;
                    Bridge moved = using.reachable();
                    if (moved == null || moved.host.equalsIgnoreCase(using.host)) throw e;
                    using = moved;
                    sessions = using.harnesses().optJSONArray("harnesses");
                }
                if (learn) using = using.learnAddresses();
                Bridge working = using;
                JSONArray found = sessions;
                runOnUiThread(() -> {
                    loading = false;
                    if (current != bridge || list == null) return;
                    learned = true;
                    if (working != current) {
                        bridge = working;
                        Ui.saveBridge(this, working);
                    }
                    status.setText(working.host + " · connected via " + working.route());
                    status.setTextColor(Ui.MUTED);
                    renderSessions(found == null ? new JSONArray() : found);
                });
            } catch (Exception e) {
                runOnUiThread(() -> {
                    loading = false;
                    if (current != bridge || status == null) return;
                    String hint = Bridge.unreachable(e) && current.alternates.isEmpty() && !Bridge.isTailscale(current.host)
                            ? "\nAway from home? Set the PC's Tailscale address in ⋯ → PC address."
                            : "";
                    status.setText(Ui.explain(e) + hint);
                    status.setTextColor(Ui.DANGER);
                });
            }
        });
    }

    private void renderSessions(JSONArray sessions) {
        list.removeAllViews();
        if (sessions.length() == 0) {
            TextView empty = Ui.text(this, "No sessions are running on the PC. Start one with the button above.", 15, Ui.MUTED);
            empty.setPadding(0, Ui.dp(this, 20), 0, 0);
            list.addView(empty);
            return;
        }
        for (int i = 0; i < sessions.length(); i++) {
            JSONObject s = sessions.optJSONObject(i);
            if (s != null) list.addView(sessionRow(s));
        }
    }

    private View sessionRow(JSONObject s) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.VERTICAL);
        int pad = Ui.dp(this, 14);
        row.setPadding(pad, pad, pad, pad);
        row.setBackground(Ui.rounded(Ui.CARD, Ui.dp(this, 12)));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, -2);
        lp.setMargins(0, 0, 0, Ui.dp(this, 10));
        row.setLayoutParams(lp);

        String name = s.optString("icon", "") + " " + s.optString("agentName", s.optString("agentType"));
        LinearLayout top = new LinearLayout(this);
        TextView agent = Ui.text(this, name.trim(), 17, Ui.TEXT);
        agent.setTypeface(android.graphics.Typeface.DEFAULT_BOLD);
        top.addView(agent, new LinearLayout.LayoutParams(0, -2, 1));
        String state = s.optString("status");
        top.addView(Ui.text(this, s.optString("label", state), 14, Ui.statusColor(state)));
        row.addView(top);

        String title = firstNonEmpty(s.optString("sessionTitle"), s.optString("lastPrompt"));
        if (!title.isEmpty()) {
            TextView t = Ui.text(this, title, 15, Ui.TEXT);
            t.setMaxLines(2);
            t.setEllipsize(android.text.TextUtils.TruncateAt.END);
            t.setPadding(0, Ui.dp(this, 6), 0, 0);
            row.addView(t);
        }
        TextView dir = Ui.text(this, s.optString("directoryDisplay"), 13, Ui.MUTED);
        dir.setPadding(0, Ui.dp(this, 4), 0, 0);
        row.addView(dir);

        String id = s.optString("id");
        row.setOnClickListener(v -> {
            Intent open = new Intent(this, SessionActivity.class);
            open.putExtra(SessionActivity.EXTRA_ID, id);
            open.putExtra(SessionActivity.EXTRA_NAME, name.trim());
            startActivity(open);
        });
        return row;
    }

    private static String firstNonEmpty(String a, String b) {
        if (a != null && !a.isEmpty() && !"null".equals(a)) return a;
        return b == null || "null".equals(b) ? "" : b;
    }

    // ---- launching ----

    private void chooseHarness() {
        Bridge current = bridge;
        Ui.IO.execute(() -> {
            try {
                JSONObject types = current.harnessTypes();
                JSONArray all = types.optJSONArray("types");
                List<String> ids = new ArrayList<>();
                List<String> labels = new ArrayList<>();
                for (int i = 0; all != null && i < all.length(); i++) {
                    JSONObject t = all.getJSONObject(i);
                    if (!t.optBoolean("available")) continue;
                    ids.add(t.getString("id"));
                    labels.add((t.optString("icon") + " " + t.optString("name")).trim());
                }
                runOnUiThread(() -> {
                    if (ids.isEmpty()) {
                        Ui.toast(this, "No harnesses are installed on the PC.");
                        return;
                    }
                    new AlertDialog.Builder(this)
                            .setTitle("Start on the PC")
                            .setItems(labels.toArray(new String[0]), (d, which) ->
                                    chooseFolder(ids.get(which), labels.get(which)))
                            .show();
                });
            } catch (Exception e) {
                Ui.toast(this, Ui.explain(e));
            }
        });
    }

    private void chooseFolder(String agentType, String label) {
        Bridge current = bridge;
        Ui.IO.execute(() -> {
            LinkedHashSet<String> folders = new LinkedHashSet<>();
            try {
                JSONObject choices = current.workspaces();
                String workspace = choices.optString("workspace");
                if (!workspace.isEmpty()) folders.add(workspace);
                JSONArray recent = choices.optJSONArray("recentDirectories");
                for (int i = 0; recent != null && i < recent.length(); i++) folders.add(recent.getString(i));
            } catch (Exception ignored) {
                // An older PC without the folder list: start in its current folder.
            }
            List<String> items = new ArrayList<>(folders);
            runOnUiThread(() -> {
                if (items.isEmpty()) {
                    launch(agentType, label, null);
                    return;
                }
                new AlertDialog.Builder(this)
                        .setTitle(label + " · folder")
                        .setItems(items.toArray(new String[0]), (d, which) -> launch(agentType, label, items.get(which)))
                        .show();
            });
        });
    }

    private void launch(String agentType, String label, String folder) {
        Bridge current = bridge;
        Ui.toast(this, "Starting " + label + " on the PC…");
        Ui.IO.execute(() -> {
            try {
                current.launch(agentType, folder);
                runOnUiThread(this::loadSessions);
            } catch (Exception e) {
                Ui.toast(this, Ui.explain(e));
            }
        });
    }
}
