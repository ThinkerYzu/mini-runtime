// Shared by the HTTP viewer and the privileged mirror sandbox. Construct in the
// destination document's realm: adopting sheets from the outer viewer is refused.
function mirrorAdoptedSheets(root, descriptors) {
  if (!descriptors) return;
  const Sheet = root.ownerDocument.defaultView.CSSStyleSheet;
  root.adoptedStyleSheets = descriptors.map(({text, media, disabled}) => {
    const sheet = new Sheet({media, disabled});
    sheet.replaceSync(text);
    return sheet;
  });
}
