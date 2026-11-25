(() => {
  const escapeHtml = (text) => text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  const rustTypes = [
    "Ipv4Header", "EthernetHeader", "ArpPacket", "VirtioNet",
    "NetDevice", "Option", "Some", "None", "Frame", "Vec",
    "Result", "usize", "u8", "u16", "u32", "u64", "i32",
  ];
  const rustKeywords = [
    "pub", "fn", "let", "mut", "if", "else", "return", "const",
    "struct", "impl", "while", "for", "in", "match", "as", "use",
    "mod", "self", "Self", "crate", "unsafe", "extern", "static",
  ];

  function markRust(text) {
    let html = escapeHtml(text);
    html = html.replace(/("([^"\\]|\\.)*")/g, '<span class="s">$1</span>');
    html = html.replace(/(\/\/.*)$/gm, '<span class="c">$1</span>');
    html = html.replace(/\b(0x[0-9a-fA-F]+|\d+u(?:8|16|32|64|size)?|\d+)\b/g, '<span class="n">$1</span>');
    html = html.replace(new RegExp(`\\b(${rustTypes.join("|")})\\b`, "g"), '<span class="t">$1</span>');
    html = html.replace(/\b([a-zA-Z_][a-zA-Z0-9_]*)\s*(?=\()/g, '<span class="f">$1</span>');
    html = html.replace(new RegExp(`\\b(${rustKeywords.join("|")})\\b`, "g"), '<span class="k">$1</span>');
    return html;
  }

  function markShell(text) {
    let html = escapeHtml(text);
    html = html.replace(/("([^"\\]|\\.)*")/g, '<span class="s">$1</span>');
    html = html.replace(/\$[a-zA-Z_][a-zA-Z0-9_]*/g, '<span class="sh-var">$&</span>');
    html = html.replace(/\b(grep|if|then|fi|echo|exit|rm|sh|cargo|qemu-system-x86_64)\b/g, '<span class="sh-cmd">$1</span>');
    html = html.replace(/\b(\d+)\b/g, '<span class="n">$1</span>');
    return html;
  }

  function highlight(pre) {
    const lang = pre.getAttribute("data-lang");
    if (pre.dataset.highlighted === "true") {
      return;
    }

    const source = pre.textContent;
    if (lang === "sh" || lang === "shell" || lang === "bash") {
      pre.innerHTML = markShell(source);
    } else if (lang === "rust" || lang === "rs") {
      pre.innerHTML = markRust(source);
    }
    pre.dataset.highlighted = "true";
  }

  function highlightAll() {
    document.querySelectorAll("pre[data-lang]").forEach(highlight);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", highlightAll);
  } else {
    highlightAll();
  }
})();
