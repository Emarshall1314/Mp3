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

/// The tab a page belongs to: its own if it has one, else Library (where everything is).
export function tabOf(route: Route, tabs: TabId[]): TabId {
  const own: Partial<Record<Route['name'], TabId>> = {
    home: 'home',
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
/// in a floating capsule with a glass lens behind the current tab, which slides (and stretches
/// as it goes) to the next one, or follows a finger dragged along the bar; Search becomes a
/// round button of its own beside the capsule, and while the page scrolls down the capsule
/// shrinks to the current tab (`collapsed`; tapping it opens the bar again).
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
  const search = useRef<HTMLButtonElement>(null);
  useLens(bar, liquid);
  useLens(search, separateSearch);

  // The lens: where it is, and a finger dragging it.
  const [lens, setLens] = useState<{ x: number; w: number } | null>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const [moving, setMoving] = useState(false);
  const dragFrom = useRef<{ x: number; moved: boolean } | null>(null);
  const dragged = useRef(false);
  const index = tabs.indexOf(current);
  useLayoutEffect(() => {
    const nav = bar.current;
    if (!nav || !liquid) return;
    const place = () => {
      const button = nav.querySelectorAll<HTMLElement>('button')[index];
      setLens(button ? { x: button.offsetLeft, w: button.offsetWidth } : null);
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(nav);
    return () => observer.disconnect();
  }, [index, liquid, collapsed, tabs.length]);
  const first = useRef(true);
  useLayoutEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    setMoving(true);
    const t = setTimeout(() => setMoving(false), 420);
    return () => clearTimeout(t);
  }, [index]);

  // The tab whose middle is nearest a finger at `clientX`.
  const tabAt = (clientX: number): TabId => {
    const buttons = [...(bar.current?.querySelectorAll<HTMLElement>('button') ?? [])];
    const gaps = buttons.map((b) => {
      const box = b.getBoundingClientRect();
      return Math.abs(clientX - (box.left + box.width / 2));
    });
    return tabs[gaps.indexOf(Math.min(...gaps))] ?? current;
  };

  const draggable = liquid && !collapsed && lens !== null;
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
          const nav = bar.current;
          if (!from || !nav || !lens) return;
          if (!from.moved && Math.abs(e.clientX - from.x) < 8) return;
          from.moved = true;
          const box = nav.getBoundingClientRect();
          const x = Math.max(
            4,
            Math.min(box.width - lens.w - 4, e.clientX - box.left - lens.w / 2),
          );
          setDrag(x);
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
        {liquid && lens && !collapsed && (
          <span
            className="phone-tab-lens"
            data-moving={moving || undefined}
            data-dragging={drag !== null || undefined}
            style={{ width: lens.w, transform: `translateX(${drag ?? lens.x}px)` }}
            aria-hidden
          />
        )}
        {tabs.map((t) => {
          const Icon = ICONS[t];
          const on = current === t;
          // Collapsed, the bar keeps only the current tab (Library while on Search).
          if (collapsed && !on && !(index < 0 && t === 'library')) return null;
          return (
            <button
              key={t}
              className={on ? 'on' : ''}
              aria-current={on ? 'page' : undefined}
              aria-label={collapsed ? `${TAB_LABELS[t]}: show all tabs` : undefined}
              onClick={() => (collapsed ? onExpand() : go(t, current, route))}
            >
              <Icon size={22} aria-hidden />
              {!collapsed && <span>{TAB_LABELS[t]}</span>}
            </button>
          );
        })}
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
