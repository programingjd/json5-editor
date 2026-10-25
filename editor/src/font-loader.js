import interUrl from '../../assets/fonts/Inter.woff2?url';
import interItalicUrl from '../../assets/fonts/Inter-ital.woff2?url';
import monoUrl from '../../assets/fonts/CommitMono.woff2?url';

async function fetchFont(family, url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Failed to load ${family} font (HTTP ${response.status})`);
  return new Uint8Array(await response.arrayBuffer());
}

export async function loadFonts() {
  const [inter, interItalic, mono] = await Promise.all([
    fetchFont('Inter', interUrl),
    fetchFont('Inter Italic', interItalicUrl),
    fetchFont('CommitMono', monoUrl),
  ]);
  // Register the fetched bytes with CSS as well as GPUI, so each font has one
  // download and no embedded or separately fetched browser copy.
  const faces = await Promise.all([
    new FontFace('Inter', inter, { style: 'normal', weight: '400', display: 'swap' }).load(),
    new FontFace('Inter', interItalic, { style: 'italic', weight: '400', display: 'swap' }).load(),
    new FontFace('CommitMono', mono, { style: 'normal', weight: '500', display: 'swap' }).load(),
  ]);
  for (const face of faces) document.fonts.add(face);
  return { inter, interItalic, mono };
}
