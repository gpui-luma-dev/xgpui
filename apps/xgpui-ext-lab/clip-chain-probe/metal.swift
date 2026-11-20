// Native Metal fragment-pipeline readback; consumes the Rust CPU fixtures.
import Foundation
import Metal
let args = CommandLine.arguments
precondition(args.count == 3, "usage: metal-probe SHADER_PATH FIXTURE_DIRECTORY")
let device = MTLCreateSystemDefaultDevice()!
print("native Metal: \(device.name)")
let library = try device.makeLibrary(source: String(contentsOfFile: args[1], encoding: .utf8), options: nil)
let descriptor = MTLRenderPipelineDescriptor()
descriptor.vertexFunction = library.makeFunction(name: "vs")
descriptor.fragmentFunction = library.makeFunction(name: "fs")
descriptor.colorAttachments[0].pixelFormat = .r32Float
let pipeline = try device.makeRenderPipelineState(descriptor: descriptor)
let queue = device.makeCommandQueue()!
let directory = URL(fileURLWithPath: args[2])
let fixtures = try String(contentsOf: directory.appendingPathComponent("names.txt"), encoding: .utf8)
    .split(separator: "\n").map { directory.appendingPathComponent(String($0)).appendingPathExtension("shapes") }
precondition(!fixtures.isEmpty, "run the Rust harness first")
var results = ""
for file in fixtures {
    let base = file.deletingPathExtension()
    let data = try Data(contentsOf: file)
    var count = UInt32(try String(contentsOf: base.appendingPathExtension("count"), encoding: .utf8))!
    precondition(count <= 8 && data.count == 512)
    let expected = try Data(contentsOf: base.appendingPathExtension("expected"))
    let records = data.withUnsafeBytes { device.makeBuffer(bytes: $0.baseAddress!, length: data.count, options: .storageModeShared)! }
    let textureDescriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .r32Float, width: 96, height: 96, mipmapped: false)
    textureDescriptor.storageMode = .shared
    textureDescriptor.usage = [.renderTarget]
    let texture = device.makeTexture(descriptor: textureDescriptor)!
    let pass = MTLRenderPassDescriptor()
    pass.colorAttachments[0].texture = texture
    pass.colorAttachments[0].loadAction = .clear
    pass.colorAttachments[0].storeAction = .store
    let start = CFAbsoluteTimeGetCurrent()
    let command = queue.makeCommandBuffer()!
    let encoder = command.makeRenderCommandEncoder(descriptor: pass)!
    encoder.setRenderPipelineState(pipeline)
    encoder.setFragmentBuffer(records, offset: 0, index: 0)
    encoder.setFragmentBytes(&count, length: 4, index: 1)
    encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
    encoder.endEncoding()
    command.commit(); command.waitUntilCompleted()
    precondition(command.status == .completed, "\(String(describing: command.error))")
    var actual = [Float](repeating: 0, count: 96*96)
    actual.withUnsafeMutableBytes { texture.getBytes($0.baseAddress!, bytesPerRow: 96*4, from: MTLRegionMake2D(0,0,96,96), mipmapLevel: 0) }
    let reference = expected.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
    let error = zip(actual, reference).map { abs($0-$1) }.max()!
    precondition(error <= 0.0625, "\(base.lastPathComponent): GPU/CPU mismatch \(error)")
    let elapsed = (CFAbsoluteTimeGetCurrent()-start)*1000
    let line = "\(base.lastPathComponent): gpu_cpu_max=\(error), submit_wait_readback_ms=\(String(format: "%.3f", elapsed))\n"
    print(line, terminator: ""); results += line
}
try results.write(to: directory.appendingPathComponent("metal-results.txt"), atomically: true, encoding: .utf8)
