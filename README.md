# CraftStudio: Unified Open-Source Creative Suite

**Photo Editing, Video Editing, and Compositing — interconnected in a single application window.**

CraftStudio unifies three pure-Rust creative tools into an integrated, zero-copy creative studio:
- **🎨 Photo**: [PhotoCraft](Photocraft) — Image editing, layers, masks, raw development, vector shapes, brushes, and native PSD engine (clean-room Photoshop equivalent).
- **🎬 Video**: [FilmCraft](filmcraft) — Non-linear video editing, multi-track timeline, Lumetri color grading, scopes, and audio mixer (clean-room Premiere Pro equivalent).
- **✨ Compositing**: [EffectCraft](effectcraft) — Motion graphics, layer compositing, keyframing, graph editor, expressions, and VFX plugins (clean-room After Effects equivalent).

---

## Why a Single Unified Software?

In proprietary suites like Adobe Creative Cloud:
- Photoshop, Premiere Pro, and After Effects run as heavy, isolated separate processes.
- Switching tools requires constant alt-tabbing and OS window management.
- Dynamic Link uses heavyweight inter-process communication (IPC) that often crashes, introduces latency, or requires separate background render engines.

**CraftStudio solves this natively:**
1. **Single Native Process & Window**: All three engines run in the exact same process memory space and share the same `wgpu` graphics context and `egui 0.36` UI framework.
2. **In-Memory Direct Link**: Instant zero-copy frame buffer exchange between timelines, compositing nodes, and raster canvas.
3. **Workspace Orchestration**:
   - Switch effortlessly between **Photo**, **Video**, and **Compositing** rooms from the top Studio Ribbon without opening another window.
   - **Split Workspaces**: Work with a Photo canvas and Video timeline simultaneously, or monitor a VFX composition side-by-side with your edit sequence.
4. **Shared Project Media Bin**: Import video, audio, PSD, or image assets once and access them across all three tools with one click.

---

## Dynamic Link & Ultra Pro Studio Suite Workflows

### 🔀 Bidirectional Dynamic Link (Video ↔ Compositor)
CraftStudio features real-time in-memory dynamic linking between **FilmCraft** (Video Editor) and **EffectCraft** (Compositor):
1. **Send Comp to Video Timeline (`✨ Send Comp to Video`)**:
   - In **EffectCraft**, click `✨ Send Comp to Video` in the top Studio Ribbon.
   - The active composition is rendered and instantly placed onto the active sequence in **FilmCraft** as a linked video track item.
2. **Instant Roundtrip Jump (`Ctrl+E` or Double-Click)**:
   - Double-click any linked composition clip in the **FilmCraft** timeline or press **`Ctrl+E`** / **`Cmd+E`** to immediately switch to **EffectCraft** with that composition loaded.
   - In **EffectCraft**, pressing **`Ctrl+E`** jumps right back to **FilmCraft** at the exact sequence playhead.
3. **Live Composition Sync (`⚡ Sync Comp`)**:
   - Make any modifications in EffectCraft (keyframe adjustments, text changes, VFX layers, masks).
   - Click `⚡ Sync Comp` (or trigger an edit): the dynamic link updates the underlying footage cache and FilmCraft's media engine instantly re-fetches the updated frames dynamically without needing any restart or manual re-import.

---

## 📸 Studio Frame Capture & Clipboard History Shelf

CraftStudio includes an integrated frame capture system and visual clipboard shelf:
- **Instant Frame Capture (`📸 Capture Frame` / `Ctrl+Shift+C`)**:
  - Captures the exact pixel buffer from the current viewport (FilmCraft playhead or EffectCraft comp).
  - Automatically copies the uncompressed bitmap directly into your **Operating System clipboard** (`arboard`) so you can paste (`Ctrl+V`) into any external application.
  - Automatically registers the frame in the **Studio Frame History Shelf**.
- **Frame History Shelf (`📋 Frame History`)**:
  - Toggle the bottom visual drawer in the Studio Ribbon to inspect all captured frames as live thumbnail cards.
  - **`🎨 Photo`**: One-click dispatch of any historical frame into **PhotoCraft** as a multi-layer retouching document for cloning, healing, blemish removal, color grading, or masking.
  - **`📋 Copy`**: Re-copy any historical frame back to the system clipboard.
  - **`💾 Save`**: Export frame as a high-resolution lossless PNG.
  - **`🗑️ Delete`**: Remove individual frames or clear history.

---

## How to Run

To run the unified CraftStudio application:

```bash
cargo run --bin craft-studio
```

For optimized release performance:
```bash
cargo run --release --bin craft-studio
```

To run individual standalone apps if needed:
- PhotoCraft: `cargo run -p photocraft`
- FilmCraft: `cargo run -p filmcraft`
- EffectCraft: `cargo run -p effectcraft`

