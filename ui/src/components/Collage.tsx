import { Artwork } from './Artwork';

/// A cover made of up to four artworks in a square (one fills it when there are fewer), with
/// an optional title over a wash of colour: mixes and playlists.
export function Collage({
  art,
  size,
  title,
  tint,
}: {
  art: string[];
  size: number;
  title?: string;
  /// Names a colour wash in main.css (`data-tint`); none for a plain darkening.
  tint?: string;
}) {
  const four = art.length >= 4;
  return (
    <div className="mix-cover" data-mix={tint} style={{ width: size, height: size }}>
      {four ? (
        art.slice(0, 4).map((a) => <Artwork key={a} hash={a} size={size / 2} alt="" />)
      ) : (
        <Artwork hash={art[0] ?? null} size={size} alt="" />
      )}
      {title && <span className="mix-cover-title">{title}</span>}
    </div>
  );
}
