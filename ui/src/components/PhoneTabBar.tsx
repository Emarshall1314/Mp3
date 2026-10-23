import {
  Clock,
  Disc3,
  FolderSync,
  House,
  Library,
  ListMusic,
  Mic2,
  Search,
  Settings,
} from 'lucide-react';
import { useLayoutEffect, useRef, useState } from 'react';
import { useLook } from '../hooks/useAppearance';
import { useLens } from '../hooks/useGlass';
import { type Route, useStore } from '../store';
import { TAB_LABELS, type TabId } from '../theme/layout';

const ICONS: Record<TabId, typeof House> = {
  home: House,
  library: Library,
  search: Search,
  albums: Disc3,
  artists: Mic2,
  songs: ListMusic,
  recent: Clock,
  sync: FolderSync,
  settings: Settings,
};

const ROUTES: Record<TabId, Route> = {
  home: { name: 'home' },
  library: { name: 'library' },
  search: { name: 'search', query: '' },
  albums: { name: 'albums' },
  artists: { name: 'artists' },
  songs: { name: 'songs' },
  recent: { name: 'recent' },
  sync: { name: 'sync' },
  settings: { name: 'settings' },
};

/// Whether a page is the top page of its tab (Home, Library, a tab's own list): there, a
/// swipe right brings back Now Playing rather than going back.
export function isTabTop(route: Route, tabs: TabId[]): boolean {
  return route.name === ROUTES[tabOf(route, tabs)].name;
}

/// The tab a page belongs to: its own if it has one, else Library (where everything is).
export function tabOf(route: Route, tabs: TabId[]): TabId {
  const own: Partial<Record<Route['name'], TabId>> = {
    home: 'home',
    mix: 'home',
    library: 'library',
    search: 'search',
    albums: 'albums',
    album: 'albums',
    artists: 'artists',
    artist: 'artists',
    songs: 'songs',
    recent: 'recent',
    sync: 'sync',
    settings: 'settings',
  };
  const tab = own[route.name];
  return tab && tabs.includes(tab) ? tab : 'library';
}

function go(t: TabId, current: TabId, route: Route) {
  const store = useStore.getState();
  // Tapping the tab you're on goes back to its top page.
  if (current === t && route.name !== ROUTES[t].name) store.navigate(ROUTES[t]);
  else if (current !== t) store.navigate(ROUTES[t]);
}

/// The phone's tabs, in the owner's order (Settings → Layout → Phone). In liquid glass they sit
/// in a floating capsule with a glass lens behind the current tab. Changing tab, the lens first
/// reaches across to the new tab, then lets go of the old one, like a drop of liquid, never
/// leaving the bar; it can also be dragged along the bar with a finger. Search becomes a round
/// button of its own beside the capsule. While the page scrolls down the capsule narrows to a
/// circle round the current tab (`collapsed`; tapping it opens the bar again): the row of tabs
/// slides so that tab's icon sits in the circle while the rest fade, all real elements in
/// motion, so the glass stays live throughout.
export function PhoneTabBar({
  liquid,
  collapsed,
  onExpand,
}: {
  liquid: boolean;
  collapsed: boolean;
  onExpand: () => void;
}) {
  const all = useLook((s) => s.look.layout.phoneTabs);
  const route = useStore((s) => s.route);
  const current = tabOf(route, all);
  const separateSearch = liquid && all.includes('search');
  const tabs = separateSearch ? all.filter((t) => t !== 'search') : all;
  const bar = useRef<HTMLElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLButtonElement>(null);
  useLens(bar, liquid);
  useLens(search, separateSearch);

  // Where each tab is in the row (measured), and so where the lens goes.
  const [spots, setSpots] = useState<{ x: number; w: number }[]>([]);
  const index = tabs.indexOf(current);
  // Collapsed, the current tab (Library while on Search) is the one in the circle.
  const shown = index < 0 ? Math.max(0, tabs.indexOf('library')) : index;
  useLayoutEffect(() => {
    const el = row.current;
    if (!el) return;
    const measure = () =>
      setSpots(
        [...el.querySelectorAll<HTMLElement>(':scope > button')].map((b) => ({
          x: b.offsetLeft,
          w: b.offsetWidth,
        })),
      );
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [tabs.length]);

  // The lens: reaching from the old tab to the new one, then settling on the new one.
  const [reach, setReach] = useState<{ x: number; w: number } | null>(null);
  const last = useRef(index);
  useLayoutEffect(() => {
    const from = spots[last.current];
    const to = spots[index];
    last.current = index;
    if (!from || !to || from === to) return;
    const left = Math.min(from.x, to.x);
    setReach({ x: left, w: Math.max(from.x + from.w, to.x + to.w) - left });
    const t = setTimeout(() => setReach(null), 150);
    return () => clearTimeout(t);
    // Only a change of tab moves the lens; re-measuring places it directly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [index]);
  const [drag, setDrag] = useState<number | null>(null);
  const dragFrom = useRef<{ x: number; moved: boolean } | null>(null);
  const dragged = useRef(false);
  const lens = index >= 0 ? spots[index] : undefined;

  // The tab whose middle is nearest a finger at `clientX`.
  const tabAt = (clientX: number): TabId => {
    const buttons = [...(row.current?.querySelectorAll<HTMLElement>(':scope > button') ?? [])];
    const gaps = buttons.map((b) => {
      const box = b.getBoundingClientRect();
      return Math.abs(clientX - (box.left + box.width / 2));
    });
    return tabs[gaps.indexOf(Math.min(...gaps))] ?? current;
  };

  // Collapsed, the row slides so the shown tab's middle is the circle's middle (27 px into the
  // 54 px inside the circle).
  const spot = spots[shown];
  const slide = collapsed && spot ? spot.x + spot.w / 2 - 27 : 0;
  const draggable = liquid && !collapsed && lens !== undefined;
  const at = drag !== null ? { x: drag, w: lens?.w ?? 0 } : (reach ?? lens);
  return (
    <>
      <nav
        ref={bar}
        className={`phone-tabs${liquid ? ' glass-lit' : ''}`}
        aria-label="Tabs"
        data-collapsed={collapsed || undefined}
        onPointerDown={(e) => {
          if (draggable) dragFrom.current = { x: e.clientX, moved: false };
        }}
        onPointerMove={(e) => {
          const from = dragFrom.current;
          const box = row.current?.getBoundingClientRect();
          if (!from || !box || !lens) return;
          if (!from.moved && Math.abs(e.clientX - from.x) < 8) return;
          from.moved = true;
          // Inside the row, so the lens never leaves the bar.
          setDrag(Math.max(0, Math.min(box.width - lens.w, e.clientX - box.left - lens.w / 2)));
        }}
        onPointerUp={(e) => {
          const from = dragFrom.current;
          dragFrom.current = null;
          if (!from?.moved) return;
          dragged.current = true;
          setTimeout(() => (dragged.current = false), 0);
          setDrag(null);
          go(tabAt(e.clientX), current, route);
        }}
        onPointerCancel={() => {
          dragFrom.current = null;
          setDrag(null);
        }}
        onClickCapture={(e) => {
          // A drag along the bar chose its tab on release; don't also click where it ended.
          if (dragged.current) e.stopPropagation();
        }}
      >
        <div
          ref={row}
          className="phone-tabs-row"
          style={liquid ? { transform: `translateX(${-slide}px)` } : undefined}
        >
          {liquid && at && (
            <span
              className="phone-tab-lens"
              data-reaching={reach !== null || undefined}
              data-dragging={drag !== null || undefined}
              style={{ width: at.w, transform: `translateX(${at.x}px)` }}
              aria-hidden
            />
          )}
          {tabs.map((t, i) => {
            const Icon = ICONS[t];
            const on = current === t;
            const hidden = collapsed && i !== shown;
            return (
              <button
                key={t}
                className={on ? 'on' : ''}
                aria-current={on ? 'page' : undefined}
                aria-hidden={hidden || undefined}
                tabIndex={hidden ? -1 : undefined}
                aria-label={collapsed && !hidden ? `${TAB_LABELS[t]}: show all tabs` : undefined}
                data-faded={hidden || undefined}
                onClick={() => (collapsed ? onExpand() : go(t, current, route))}
              >
                <Icon size={22} aria-hidden />
                <span>{TAB_LABELS[t]}</span>
              </button>
            );
          })}
        </div>
      </nav>
      {separateSearch && (
        <button
          ref={search}
          className={`phone-search-button glass-lit${current === 'search' ? ' on' : ''}`}
          aria-label="Search"
          aria-current={current === 'search' ? 'page' : undefined}
          onClick={() => go('search', current, route)}
        >
          <Search size={22} aria-hidden />
        </button>
      )}
    </>
  );
}
