#if os(iOS)
import Foundation

// The native XMP code uses the macOS Foundation XML tree API, which is not
// available on iOS. This deliberately implements only the subset it uses.
class XMLNode: NSObject, NSCopying {
    fileprivate enum Kind { case document, element, attribute, namespace, text, comment, instruction }
    fileprivate let kind: Kind
    var name: String?
    weak var parent: XMLNode?
    fileprivate var storage: [XMLNode] = []
    fileprivate var value: String?
    fileprivate init(_ kind: Kind, name: String? = nil, value: String? = nil) {
        self.kind = kind; self.name = name; self.value = value
    }
    var children: [XMLNode]? { kind == .document || kind == .element ? storage : nil }
    var localName: String? { name.map { String($0.split(separator: ":", maxSplits: 1).last ?? "") } }
    var uri: String? {
        guard kind == .element || kind == .attribute, let name = name else { return nil }
        let parts = name.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
        if parts.count == 1 && kind == .attribute { return nil }
        let prefix = parts.count == 2 ? String(parts[0]) : ""
        if prefix == "xml" { return "http://www.w3.org/XML/1998/namespace" }
        let start = kind == .element ? self as? XMLElement : parent as? XMLElement
        return start?.namespace(prefix)?.stringValue
    }
    var stringValue: String? {
        get {
            if kind == .element || kind == .document {
                return storage.compactMap { $0.kind == .element || $0.kind == .text ? $0.stringValue : nil }.joined()
            }
            return value
        }
        set {
            if kind == .element || kind == .document {
                for child in storage { child.parent = nil }
                storage.removeAll()
                if let newValue = newValue { append(XMLNode(.text, value: newValue)) }
            } else { value = newValue }
        }
    }
    fileprivate func append(_ child: XMLNode) { child.detach(); child.parent = self; storage.append(child) }
    func detach() {
        guard let p = parent else { return }
        p.storage.removeAll { $0 === self }
        if let element = p as? XMLElement {
            element.attrs.removeAll { $0 === self }; element.ns.removeAll { $0 === self }
        }
        parent = nil
    }
    func copy(with zone: NSZone? = nil) -> Any {
        let result: XMLNode
        if let element = self as? XMLElement {
            let e = XMLElement(name: element.name ?? "")
            for n in element.ns { e.addNamespace(n.copy() as! XMLNode) }
            for a in element.attrs { e.addAttribute(a.copy() as! XMLNode) }
            result = e
        } else { result = XMLNode(kind, name: name, value: value) }
        for child in storage { result.append(child.copy() as! XMLNode) }
        return result
    }
}

final class XMLElement: XMLNode {
    fileprivate var attrs: [XMLNode] = []
    fileprivate var ns: [XMLNode] = []
    fileprivate init(name: String) { super.init(.element, name: name) }
    var attributes: [XMLNode]? { attrs.isEmpty ? nil : attrs }
    var namespaces: [XMLNode]? { ns.isEmpty ? nil : ns }
    fileprivate func addAttribute(_ node: XMLNode) { node.detach(); node.parent = self; attrs.append(node) }
    func addNamespace(_ node: XMLNode) { node.detach(); node.parent = self; ns.append(node) }
    func addChild(_ node: XMLNode) { append(node) }
    fileprivate func namespace(_ prefix: String) -> XMLNode? {
        ns.first { ($0.name ?? "") == prefix } ?? (parent as? XMLElement)?.namespace(prefix)
    }
    func resolveNamespace(forName name: String) -> XMLNode? {
        let parts = name.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
        return namespace(parts.count == 2 ? String(parts[0]) : "")
    }
}

final class XMLDocument: XMLNode {
    struct Options: OptionSet {
        let rawValue: Int
        static let nodePreserveAll = Options(rawValue: 1)
        static let nodeLoadExternalEntitiesNever = Options(rawValue: 2)
    }
    init(data: Data, options: Options) throws {
        super.init(.document)
        let builder = XMLTreeBuilder(document: self)
        let parser = XMLParser(data: data)
        parser.shouldProcessNamespaces = false
        parser.shouldResolveExternalEntities = false
        parser.delegate = builder
        guard parser.parse(), !builder.rejected, rootElement() != nil else {
            throw parser.parserError ?? NSError(domain: "XMLCompat", code: 1, userInfo: [NSLocalizedDescriptionKey: "Invalid or unsupported XML document"])
        }
    }
    convenience init(xmlString: String, options: Options) throws { try self.init(data: Data(xmlString.utf8), options: options) }
    func rootElement() -> XMLElement? { storage.compactMap { $0 as? XMLElement }.first }
    func setRootElement(_ element: XMLElement) {
        if let old = rootElement(), let index = storage.firstIndex(where: { $0 === old }) {
            element.detach(); old.parent = nil; element.parent = self; storage[index] = element
        } else { append(element) }
    }
    var xmlString: String { String(decoding: xmlData(options: []), as: UTF8.self) }
    func xmlData(options: Options) -> Data { Data(storage.map { serialize($0) }.joined().utf8) }
}

private func escaped(_ s: String, attribute: Bool = false) -> String {
    var s = s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;")
    if attribute { s = s.replacingOccurrences(of: "\"", with: "&quot;").replacingOccurrences(of: "\r", with: "&#13;").replacingOccurrences(of: "\n", with: "&#10;").replacingOccurrences(of: "\t", with: "&#9;") }
    else { s = s.replacingOccurrences(of: "\r", with: "&#13;") }
    return s
}
private func serialize(_ node: XMLNode) -> String {
    switch node.kind {
    case .element:
        let e = node as! XMLElement, name = e.name ?? ""
        let ns = e.ns.map { " xmlns" + (($0.name ?? "").isEmpty ? "" : ":" + $0.name!) + "=\"" + escaped($0.value ?? "", attribute: true) + "\"" }.joined()
        let attrs = e.attrs.map { " " + ($0.name ?? "") + "=\"" + escaped($0.value ?? "", attribute: true) + "\"" }.joined()
        return "<" + name + ns + attrs + ">" + e.storage.map { serialize($0) }.joined() + "</" + name + ">"
    case .text: return escaped(node.value ?? "")
    case .comment: return "<!--" + (node.value ?? "") + "-->"
    case .instruction: return "<?" + (node.name ?? "") + ((node.value ?? "").isEmpty ? "" : " " + node.value!) + "?>"
    case .document: return node.storage.map { serialize($0) }.joined()
    default: return ""
    }
}

private final class XMLTreeBuilder: NSObject, XMLParserDelegate {
    let document: XMLDocument
    var current: XMLNode
    var rejected = false
    init(document: XMLDocument) { self.document = document; self.current = document }
    func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes: [String: String]) {
        let e = XMLElement(name: elementName)
        for (name, value) in attributes.sorted(by: { $0.key < $1.key }) {
            if name == "xmlns" { e.addNamespace(XMLNode(.namespace, name: "", value: value)) }
            else if name.hasPrefix("xmlns:") { e.addNamespace(XMLNode(.namespace, name: String(name.dropFirst(6)), value: value)) }
            else { e.addAttribute(XMLNode(.attribute, name: name, value: value)) }
        }
        current.append(e); current = e
    }
    func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) { current = current.parent ?? document }
    func parser(_ parser: XMLParser, foundCharacters string: String) { current.append(XMLNode(.text, value: string)) }
    func parser(_ parser: XMLParser, foundCDATA CDATABlock: Data) { current.append(XMLNode(.text, value: String(decoding: CDATABlock, as: UTF8.self))) }
    func parser(_ parser: XMLParser, foundComment comment: String) { current.append(XMLNode(.comment, value: comment)) }
    func parser(_ parser: XMLParser, foundProcessingInstructionWithTarget target: String, data: String?) { current.append(XMLNode(.instruction, name: target, value: data)) }
    // XMP needs no DTD. Reject declarations rather than silently losing them.
    func parser(_ parser: XMLParser, foundInternalEntityDeclarationWithName name: String, value: String?) { rejected = true; parser.abortParsing() }
    func parser(_ parser: XMLParser, foundExternalEntityDeclarationWithName name: String, publicID: String?, systemID: String?) { rejected = true; parser.abortParsing() }
    func parser(_ parser: XMLParser, foundElementDeclarationWithName elementName: String, model: String) { rejected = true; parser.abortParsing() }
    func parser(_ parser: XMLParser, foundAttributeDeclarationWithName attributeName: String, forElement elementName: String, type: String?, defaultValue: String?) { rejected = true; parser.abortParsing() }
    func parser(_ parser: XMLParser, foundNotationDeclarationWithName name: String, publicID: String?, systemID: String?) { rejected = true; parser.abortParsing() }
    func parser(_ parser: XMLParser, foundUnparsedEntityDeclarationWithName name: String, publicID: String?, systemID: String?, notationName: String?) { rejected = true; parser.abortParsing() }
}
#endif
