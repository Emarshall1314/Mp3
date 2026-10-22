import { open } from '@tauri-apps/plugin-dialog';
import { ImagePlus, X } from 'lucide-react';
import { api, artUrl } from '../api';
import { attempt, useStore } from '../store';
import type { PictureEdit, PictureRef, TrackTags } from '../types';

export const PICTURE_TYPES: [number, string][] = [
  [3, 'Front Cover'],
  [4, 'Back Cover'],
  [5, 'Leaflet'],
  [6, 'Media'],
  [8, 'Artist'],
  [10, 'Band'],
  [7, 'Lead Artist'],
  [18, 'Illustration'],
  [19, 'Band Logo'],
  [20, 'Label Logo'],
  [0, 'Other'],
];

const typeName = (t: number) => PICTURE_TYPES.find(([n]) => n === t)?.[1] ?? 'Other';

async function pickImage(): Promise<string | null> {
  const path = await open({
    title: 'Choose an Image',
    filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif', 'bmp'] }],
  });
  return typeof path === 'string' ? path : null;
}

/// The pictures one song's edit starts from: its images that are readable.
export function initialPictures(t: TrackTags): PictureRef[] {
  return t.pictures.flatMap((p) =>
    p.hash ? [{ pictureType: p.pictureType, hash: p.hash, description: p.description }] : [],
  );
}

/// Artwork: for one song every picture (add, retype, describe, remove); for several, one cover
/// for all or none. Changes are written with Save, like the other fields.
export function ArtworkEditor({
  tags,
  edit,
  setEdit,
}: {
  tags: TrackTags[];
  edit: PictureEdit | null;
  setEdit: (e: PictureEdit | null) => void;
}) {
  const showToast = useStore((s) => s.showToast);
  const first = tags[0];
  if (!first) return null;
  const many = tags.length > 1;

  const add = async (asFront: boolean) => {
    const path = await pickImage();
    if (!path) return;
    const art = await attempt(api.importArtwork(path));
    if (!art) return;
    if (many) {
      setEdit({ kind: 'setFront', hash: art.hash });
      return;
    }
    const current = edit?.kind === 'replace' ? edit.pictures : initialPictures(first);
    const hasFront = current.some((p) => p.pictureType === 3);
    const added = { pictureType: asFront || !hasFront ? 3 : 0, hash: art.hash, description: '' };
    setEdit({ kind: 'replace', pictures: [...current, added] });
  };

  const saveToFolder = async () => {
    const path = await attempt(api.saveCoverToFolder(first.trackId));
    if (path) showToast(`Saved ${path.split('/').at(-1) ?? 'the cover'} beside the song`);
  };

  if (many) {
    const covers = tags.map((t) => t.pictures.find((p) => p.pictureType === 3)?.hash ?? null);
    const shared = covers.every((c) => c === covers[0]) ? covers[0] : undefined;
    const withArt = tags.filter((t) => t.pictures.length > 0).length;
    return (
      <div className="artwork-editor">
        <div className="artwork-summary">
          {edit?.kind === 'setFront' ? (
            <img src={artUrl(edit.hash, 160)} alt="New cover" width={160} height={160} />
          ) : shared ? (
            <img src={artUrl(shared, 160)} alt="Cover" width={160} height={160} />
          ) : (
            <div className="artwork-placeholder" style={{ width: 160, height: 160 }} />
          )}
          <div>
            <p>
              {edit?.kind === 'setFront'
                ? `On save, this becomes the front cover of all ${tags.length} songs; their other pictures stay.`
                : edit?.kind === 'removeAll'
                  ? `On save, all artwork is removed from ${tags.length} songs.`
                  : shared === undefined
                    ? 'The songs have different covers.'
                    : shared
                      ? `All ${tags.length} songs have this cover.`
                      : `${withArt} of ${tags.length} songs have artwork.`}
            </p>
            <div className="action-row">
              <button className="pill" onClick={() => void add(true)}>
                <ImagePlus size={14} aria-hidden /> Set Cover for All…
              </button>
              <button
                className="pill"
                onClick={() => setEdit({ kind: 'removeAll' })}
                disabled={withArt === 0}
              >
                Remove All Artwork
              </button>
              {edit && (
                <button className="link" onClick={() => setEdit(null)}>
                  Keep the current artwork
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    );
  }

  const pictures = edit?.kind === 'replace' ? edit.pictures : initialPictures(first);
  const damaged = first.pictures.filter((p) => !p.hash).length;
  const info = (hash: string) => first.pictures.find((p) => p.hash === hash);
  const update = (i: number, patch: Partial<PictureRef>) =>
    setEdit({
      kind: 'replace',
      pictures: pictures.map((p, j) => (j === i ? { ...p, ...patch } : p)),
    });

  return (
    <div className="artwork-editor">
      {pictures.length === 0 && <p className="muted">No artwork in this file.</p>}
      <ul className="picture-list">
        {pictures.map((p, i) => {
          const meta = info(p.hash);
          return (
            <li key={`${p.hash}-${i}`}>
              <img src={artUrl(p.hash, 128)} alt={typeName(p.pictureType)} width={96} height={96} />
              <div className="picture-fields">
                <select
                  aria-label="Picture type"
                  value={p.pictureType}
                  onChange={(e) => update(i, { pictureType: Number(e.target.value) })}
                >
                  {PICTURE_TYPES.map(([n, label]) => (
                    <option key={n} value={n}>
                      {label}
                    </option>
                  ))}
                </select>
                <input
                  type="text"
                  aria-label="Description"
                  placeholder="Description"
                  value={p.description}
                  onChange={(e) => update(i, { description: e.target.value })}
                />
                {meta && (
                  <span className="muted small">
                    {meta.width}×{meta.height} · {Math.round(meta.bytes / 1024).toLocaleString()} KB
                  </span>
                )}
              </div>
              <button
                className="icon-button small"
                aria-label={`Remove ${typeName(p.pictureType)}`}
                onClick={() =>
                  setEdit({ kind: 'replace', pictures: pictures.filter((_, j) => j !== i) })
                }
              >
                <X size={14} />
              </button>
            </li>
          );
        })}
      </ul>
      {damaged > 0 && (
        <p className="small warn">
          {damaged === 1 ? 'One picture is' : `${damaged} pictures are`} damaged and can’t be shown;
          changing the artwork removes {damaged === 1 ? 'it' : 'them'}.
        </p>
      )}
      <div className="action-row">
        <button className="pill" onClick={() => void add(false)}>
          <ImagePlus size={14} aria-hidden /> Add Image…
        </button>
        <button
          className="pill"
          disabled={first.pictures.length === 0}
          onClick={() => void saveToFolder()}
          title="Save the front cover as cover.jpg beside the song (never replaces an existing file)"
        >
          Save Cover to Folder
        </button>
        {edit && (
          <button className="link" onClick={() => setEdit(null)}>
            Undo artwork changes
          </button>
        )}
      </div>
    </div>
  );
}
