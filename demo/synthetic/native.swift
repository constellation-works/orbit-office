// macOS-only rendering and PDF extraction for the synthetic pack. No UI or capture.
import AppKit
import PDFKit

enum DemoError: Error { case arguments, invalidPDF, render }

func main() throws {
    let args = CommandLine.arguments
    if args.count == 7 && args[1] == "pdf" && Array(args[2...4]) == ["-layout", "-enc", "UTF-8"] && args[6] == "-" {
        guard let document = PDFDocument(url: URL(fileURLWithPath: args[5])) else { throw DemoError.invalidPDF }
        for index in 0..<document.pageCount {
            guard let page = document.page(at: index), let text = page.string else { throw DemoError.invalidPDF }
            FileHandle.standardOutput.write(Data((text + "\n\u{c}").utf8))
        }
    } else if args.count == 5 && args[1] == "frame" {
        let text = try String(contentsOfFile: args[2], encoding: .utf8)
        guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 1280, pixelsHigh: 720,
                  bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                  colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
              let context = NSGraphicsContext(bitmapImageRep: bitmap), let font = NSFont(name: "Menlo", size: 26)
        else { throw DemoError.render }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = context
        let light = args[4] == "light"
        (light ? NSColor(calibratedRed: 0.91, green: 0.94, blue: 0.93, alpha: 1)
               : NSColor(calibratedRed: 16/255, green: 28/255, blue: 44/255, alpha: 1)).setFill()
        NSRect(x: 0, y: 0, width: 1280, height: 720).fill()
        let style = NSMutableParagraphStyle()
        style.lineSpacing = 10
        (text as NSString).draw(in: NSRect(x: 55, y: 40, width: 1170, height: 625), withAttributes: [
            .font: font, .foregroundColor: light ? NSColor.black : NSColor(calibratedRed: 203/255, green: 231/255, blue: 236/255, alpha: 1),
            .paragraphStyle: style])
        NSGraphicsContext.restoreGraphicsState()
        guard let png = bitmap.representation(using: .png, properties: [:]) else { throw DemoError.render }
        try png.write(to: URL(fileURLWithPath: args[3]))
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: args[3])
    } else { throw DemoError.arguments }
}

do { try main() } catch { exit(1) }
