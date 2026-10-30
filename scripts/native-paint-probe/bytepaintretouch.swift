import AppKit

func byteCloneRecordUI() throws -> [String: Any] {
    let trials = try [bytePaintGestureTrial(.clone, name: "clone-pressure", variant: "pressure"),
        bytePaintGestureTrial(.clone, name: "clone-held", variant: "soft", held: true),
        bytePaintGestureTrial(.clone, name: "clone-mouse-soft", variant: "soft"),
        bytePaintGestureTrial(.clone, name: "clone-mouse-flow", variant: "flow"),
        bytePaintGestureTrial(.clone, name: "clone-mouse-round", variant: "round"),
        bytePaintGestureTrial(.clone, name: "clone-transformed-unaligned", variant: "soft",
                              optionVariant: "source_transform"),
        bytePaintGestureTrial(.clone, name: "clone-current", variant: "soft",
                              optionVariant: "sample_current"),
        bytePaintGestureTrial(.clone, name: "clone-current-below", variant: "soft",
                              optionVariant: "sample_below"),
        bytePaintGestureTrial(.clone, name: "clone-ignore-adjustments", variant: "soft",
                              optionVariant: "ignore_adjustments"),
        bytePaintGestureTrial(.clone, name: "clone-new-layer", variant: "soft",
                              optionVariant: "new_layer"),
        bytePaintGestureTrial(.clone, name: "clone-expand", variant: "soft",
                              optionVariant: "expand"),
        bytePaintGestureTrial(.clone, name: "clone-multiply", variant: "soft",
                              optionVariant: "blend"),
        bytePaintGestureTrial(.clone, name: "clone-fractional", variant: "soft",
                              optionVariant: "fractional"),
        bytePaintGestureTrial(.clone, name: "clone-mid-drag-options", variant: "soft",
                              midDragOptions: true),
        bytePaintGestureTrial(.clone, name: "clone-p3", variant: "soft", p3: true)]
    return ["case": "byte-clone-record-ui", "trials": trials]
}

func byteHealRecordUI() throws -> [String: Any] {
    let trials = try [bytePaintGestureTrial(.heal, name: "heal-pressure", variant: "pressure"),
        bytePaintGestureTrial(.heal, name: "heal-held", variant: "soft", held: true),
        bytePaintGestureTrial(.heal, name: "heal-mouse-soft", variant: "soft"),
        bytePaintGestureTrial(.heal, name: "heal-mouse-round", variant: "round"),
        bytePaintGestureTrial(.heal, name: "heal-transformed-unaligned", variant: "soft",
                              optionVariant: "source_transform"),
        bytePaintGestureTrial(.heal, name: "heal-aligned-carried-transform", variant: "soft",
                              optionVariant: "aligned_heal", alignedHealCarry: true),
        bytePaintGestureTrial(.heal, name: "heal-current", variant: "soft",
                              optionVariant: "sample_current"),
        bytePaintGestureTrial(.heal, name: "heal-current-below", variant: "soft",
                              optionVariant: "sample_below"),
        bytePaintGestureTrial(.heal, name: "heal-ignore-adjustments", variant: "soft",
                              optionVariant: "ignore_adjustments"),
        bytePaintGestureTrial(.heal, name: "heal-new-layer", variant: "soft",
                              optionVariant: "new_layer"),
        bytePaintGestureTrial(.heal, name: "heal-expand", variant: "soft",
                              optionVariant: "expand"),
        bytePaintGestureTrial(.heal, name: "heal-mid-drag-options", variant: "soft",
                              midDragOptions: true),
        bytePaintGestureTrial(.heal, name: "heal-p3", variant: "soft", p3: true)]
    return ["case": "byte-heal-record-ui", "trials": trials]
}

func byteSpotHealRecordUI() throws -> [String: Any] {
    let trials = try [bytePaintGestureTrial(.spotHeal, name: "spot-pressure", variant: "pressure"),
        bytePaintGestureTrial(.spotHeal, name: "spot-held", variant: "soft", held: true),
        bytePaintGestureTrial(.spotHeal, name: "spot-mouse-soft", variant: "soft"),
        bytePaintGestureTrial(.spotHeal, name: "spot-mouse-round", variant: "round"),
        bytePaintGestureTrial(.spotHeal, name: "spot-current", variant: "soft",
                              optionVariant: "sample_current"),
        bytePaintGestureTrial(.spotHeal, name: "spot-current-below", variant: "soft",
                              optionVariant: "sample_below"),
        bytePaintGestureTrial(.spotHeal, name: "spot-ignore-adjustments", variant: "soft",
                              optionVariant: "ignore_adjustments"),
        bytePaintGestureTrial(.spotHeal, name: "spot-new-layer", variant: "soft",
                              optionVariant: "new_layer"),
        bytePaintGestureTrial(.spotHeal, name: "spot-expand", variant: "soft",
                              optionVariant: "expand"),
        bytePaintGestureTrial(.spotHeal, name: "spot-p3", variant: "soft", p3: true)]
    return ["case": "byte-spot-heal-record-ui", "trials": trials]
}

func byteDodgeRecordUI() throws -> [String: Any] {
    let trials = try [bytePaintGestureTrial(.dodge, name: "dodge-pressure", variant: "pressure"),
        bytePaintGestureTrial(.dodge, name: "dodge-held", variant: "soft", held: true),
        bytePaintGestureTrial(.dodge, name: "dodge-mouse-soft", variant: "soft"),
        bytePaintGestureTrial(.dodge, name: "dodge-mouse-flow", variant: "flow"),
        bytePaintGestureTrial(.dodge, name: "dodge-mouse-round", variant: "round"),
        bytePaintGestureTrial(.dodge, name: "dodge-shadows", variant: "soft", range: 0),
        bytePaintGestureTrial(.dodge, name: "dodge-midtones", variant: "soft", range: 1),
        bytePaintGestureTrial(.dodge, name: "dodge-highlights", variant: "soft", range: 2),
        bytePaintGestureTrial(.dodge, name: "burn-shadows", variant: "soft", range: 0,
                              burn: true),
        bytePaintGestureTrial(.dodge, name: "burn-midtones", variant: "soft", range: 1,
                              burn: true),
        bytePaintGestureTrial(.dodge, name: "burn-highlights", variant: "soft",
                              range: 2, burn: true),
        bytePaintGestureTrial(.dodge, name: "dodge-pixel-held", variant: "pressure",
                              held: true, optionVariant: "pixel_held"),
        bytePaintGestureTrial(.dodge, name: "dodge-p3", variant: "soft", p3: true)]
    return ["case": "byte-dodge-record-ui", "trials": trials]
}
