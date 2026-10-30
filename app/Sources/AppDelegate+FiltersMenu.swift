import AppKit

extension AppDelegate {
    /// Filters: the true PIXEL filters. Every tone and colour adjustment
    /// moved to Image > Adjustments when the batch grew past what one flat
    /// menu can carry — same selectors, same shortcuts, same behaviour.
    func filtersMenu() -> NSMenu {
        let menu = NSMenu(title: "Filters")
        // ⌃F. The title becomes Repeat "Sharpen"
        // when there is something to repeat (EditorViewController+Actions
        // .validateActionsItem), which also disables it while text is being
        // edited: NSTextView binds ⌃F to moveForward:, and a menu key
        // equivalent is resolved AHEAD of the first responder — the same trap
        // Edit ▸ Clear's bare ⌫ carries, and answered the same way.
        menu.addItem(
            item(
                "Repeat Last Filter",
                #selector(EditorViewController.repeatLastFilter(_:)), "f", [.control]))
        menu.addItem(.separator())
        // ⇧⌘X: a modal workspace, not a slider sheet.
        menu.addItem(
            item("Liquify…", #selector(EditorViewController.showLiquify(_:)), "x", [.command, .shift]))
        // ⇧⌘A, which was free: the Develop panel's global
        // controls as a one-shot destructive filter, directly above
        // Lens Correction….
        menu.addItem(
            item("Develop Filter…", #selector(EditorViewController.showDevelopFilter(_:)), "a",
                 [.command, .shift]))
        menu.addItem(
            item(
                "Lens Correction…", #selector(EditorViewController.showLensCorrection(_:)), "r",
                [.command, .shift]))
        // Texture, Clarity and Dehaze in one sheet; no key equivalent.
        menu.addItem(item("Presence…", #selector(EditorViewController.showPresence(_:))))
        // Neural Filters: local models from Settings ▸ Models.
        let neural = NSMenu(title: "Neural Filters")
        neural.addItem(item("Depth Blur…", #selector(EditorViewController.showDepthBlur(_:))))
        neural.addItem(item("Super Zoom…", #selector(EditorViewController.showSuperZoom(_:))))
        neural.addItem(item("JPEG Artifacts Removal…", #selector(EditorViewController.showJpegArtifactsRemoval(_:))))
        neural.addItem(item("Colorize…", #selector(EditorViewController.showColorize(_:))))
        menu.addItem(submenuItem(neural))
        menu.addItem(.separator())
        // The submenu grouping. Selectors are unchanged, so custom
        // shortcuts (keyed on selector) and the Command Palette (which walks
        // submenus) keep finding every item.
        let blur = NSMenu(title: "Blur")
        blur.addItem(item("Box Blur…", #selector(EditorViewController.showBoxBlur(_:))))
        blur.addItem(item("Gaussian Blur…", #selector(EditorViewController.showBlur(_:))))
        blur.addItem(item("Lens Blur…", #selector(EditorViewController.showLensBlur(_:))))
        blur.addItem(item("Motion Blur…", #selector(EditorViewController.showMotionBlur(_:))))
        blur.addItem(item("Radial Blur…", #selector(EditorViewController.showRadialBlur(_:))))
        blur.addItem(item("Surface Blur…", #selector(EditorViewController.showSurfaceBlur(_:))))
        blur.addItem(item("Tilt-Shift…", #selector(EditorViewController.showTiltShift(_:))))
        menu.addItem(submenuItem(blur))
        let noise = NSMenu(title: "Noise")
        noise.addItem(item("Add Grain…", #selector(EditorViewController.showAddGrain(_:))))
        noise.addItem(item("Add Noise…", #selector(EditorViewController.showAddNoise(_:))))
        noise.addItem(item("Dust & Scratches…", #selector(EditorViewController.showDustAndScratches(_:))))
        noise.addItem(item("Reduce Noise…", #selector(EditorViewController.showReduceNoise(_:))))
        menu.addItem(submenuItem(noise))
        let other = NSMenu(title: "Other")
        other.addItem(item("High Pass…", #selector(EditorViewController.showHighPass(_:))))
        other.addItem(item("Offset…", #selector(EditorViewController.showOffset(_:))))
        menu.addItem(submenuItem(other))
        let pixelate = NSMenu(title: "Pixelate")
        pixelate.addItem(item("Pixelate…", #selector(EditorViewController.showPixelate(_:))))
        menu.addItem(submenuItem(pixelate))
        // No dialog: the swatches and a fresh seed (⌥: high contrast).
        let render = NSMenu(title: "Render")
        render.addItem(item("Clouds", #selector(EditorViewController.applyClouds(_:))))
        render.addItem(item("Difference Clouds", #selector(EditorViewController.applyDifferenceClouds(_:))))
        menu.addItem(submenuItem(render))
        let sharpen = NSMenu(title: "Sharpen")
        sharpen.addItem(item("Sharpen", #selector(EditorViewController.applySharpen(_:))))
        // Reduce Noise, the blur to remove and the tonal fades in one sheet.
        sharpen.addItem(item("Smart Sharpen…", #selector(EditorViewController.showSmartSharpen(_:))))
        sharpen.addItem(item("Unsharp Mask…", #selector(EditorViewController.showUnsharpMask(_:))))
        menu.addItem(submenuItem(sharpen))
        let stylize = NSMenu(title: "Stylize")
        stylize.addItem(item("Edge Detect", #selector(EditorViewController.applyEdgeDetect(_:))))
        stylize.addItem(item("Emboss", #selector(EditorViewController.applyEmboss(_:))))
        menu.addItem(submenuItem(stylize))
        menu.addItem(.separator())
        // The AUTOMATIC red-eye pass: Vision's face landmarks find the eyes
        // and the same core op runs on each. The Red Eye tool's drag
        // rectangle is the manual route.
        menu.addItem(
            item("Remove Red Eye", #selector(EditorViewController.removeRedEye(_:))))
        return menu
    }
}
