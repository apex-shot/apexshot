// SPDX-License-Identifier: AGPL-3.0-or-later

// Mutter exposes only cursor sprite size and hotspot, while the theme files
// provide the named cursor signatures. Unreadable or unmatched signatures use
// the geometry heuristic as a safe fallback.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const IMAGE_CHUNK_TYPE = 0xfffd0002;
const SETTINGS_SCHEMA = 'org.gnome.desktop.interface';
const SHAPE_NAMES = [
    ['crosshair', ['crosshair', 'cross', 'cross_reverse', 'tcross', 'diamond_cross', 'dotbox', 'dot_box_mask', 'draped_box', 'cell', 'plus', 'color-picker']],
    ['move', ['fleur', 'move', 'all-scroll', 'size_all', 'closedhand', 'grabbing', 'dnd-move']],
    ['text', ['text', 'xterm', 'ibeam', 'vertical-text']],
    ['default', ['left_ptr', 'default', 'arrow', 'top_left_arrow', 'left-arrow']],
    ['resize_h', ['ew-resize', 'col-resize', 'e-resize', 'w-resize', 'sb_h_double_arrow', 'h_double_arrow', 'left_side', 'right_side', 'size_hor', 'split_h']],
    ['resize_v', ['ns-resize', 'row-resize', 'n-resize', 's-resize', 'sb_v_double_arrow', 'v_double_arrow', 'double_arrow', 'top_side', 'bottom_side', 'size_ver', 'split_v']],
    ['resize_diag', ['nwse-resize', 'nesw-resize', 'bd_double_arrow', 'fd_double_arrow', 'nw-resize', 'ne-resize', 'sw-resize', 'se-resize', 'size_fdiag', 'size_bdiag']],
    ['wait', ['wait', 'watch', 'progress', 'half-busy', 'left_ptr_watch']],
    ['hand', ['pointer', 'hand2', 'hand', 'pointing_hand', 'hand1', 'openhand', 'grab']],
];
const CANONICAL_NAMES = {
    crosshair: 'crosshair',
    move: 'fleur',
    text: 'text',
    default: 'left_ptr',
    resize_h: 'ew-resize',
    resize_v: 'ns-resize',
    resize_diag: 'nwse-resize',
    wait: 'wait',
    hand: 'pointer',
};
const SHAPE_PRIORITY = new Map(SHAPE_NAMES.map(([shape], index) => [shape, index]));

let settingsInitialized = false;
let settings = null;
let settingsSnapshot = null;
let themeCache = null;
let mismatchInvalidated = false;
const reportedFailures = new Set();

function logFailure(kind, error) {
    if (reportedFailures.has(kind))
        return;

    reportedFailures.add(kind);
    if (typeof console !== 'undefined' && typeof console.warn === 'function')
        console.warn(`Cursor classification ${kind} failed: ${error}`);
}

function invalidateThemeCache() {
    themeCache = null;
    mismatchInvalidated = false;
}

function readSettingsSnapshot() {
    try {
        return {
            theme: settings.get_string('cursor-theme'),
            cursorSize: settings.get_int('cursor-size'),
        };
    } catch (error) {
        logFailure('settings-read', error);
        return null;
    }
}

function initializeSettings() {
    if (settingsInitialized)
        return settingsSnapshot;

    settingsInitialized = true;
    try {
        const schemaSource = Gio.SettingsSchemaSource.get_default();
        const schema = schemaSource?.lookup(SETTINGS_SCHEMA, true);
        if (!schema)
            return null;

        settings = new Gio.Settings({settings_schema: schema});
        settingsSnapshot = readSettingsSnapshot();
        settings.connect('changed::cursor-theme', () => {
            settingsSnapshot = readSettingsSnapshot();
            invalidateThemeCache();
        });
        settings.connect('changed::cursor-size', () => {
            settingsSnapshot = readSettingsSnapshot();
            invalidateThemeCache();
        });
    } catch (error) {
        settings = null;
        settingsSnapshot = null;
        logFailure('settings-init', error);
    }

    return settingsSnapshot;
}

function joinPath(...parts) {
    return parts.join('/').replace(/\/+/g, '/');
}

function getIconRoots() {
    const home = GLib.get_home_dir();
    const dataHome = GLib.getenv('XDG_DATA_HOME') || joinPath(home, '.local/share');
    const dataDirs = GLib.getenv('XDG_DATA_DIRS') || '/usr/local/share:/usr/share';
    const roots = [joinPath(dataHome, 'icons'), joinPath(home, '.icons')];
    for (const directory of dataDirs.split(':')) {
        if (directory)
            roots.push(joinPath(directory, 'icons'));
    }
    roots.push('/usr/share/icons');
    return [...new Set(roots)];
}

function fileType(path) {
    return Gio.File.new_for_path(path).query_file_type(Gio.FileQueryInfoFlags.NONE, null);
}

function readFile(path) {
    const [success, contents] = Gio.File.new_for_path(path).load_contents(null);
    if (!success)
        throw new Error(`Unable to read ${path}`);
    return contents;
}

function readExact(stream, fileSize, offset, length) {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) ||
        offset < 0 || length < 0 || offset > fileSize - length)
        throw new Error('Xcursor read exceeds file bounds');
    if (!stream.seek(offset, GLib.SeekType.SET, null))
        throw new Error('Unable to seek within Xcursor file');

    const data = stream.read_bytes(length, null).get_data();
    if (data.byteLength !== length)
        throw new Error('Truncated Xcursor file');
    return data;
}

function parseXcursorFile(stream, fileSize) {
    if (!Number.isSafeInteger(fileSize) || fileSize < 4)
        throw new Error('Invalid Xcursor header');

    const headerLength = Math.min(fileSize, 16);
    const header = readExact(stream, fileSize, 0, headerLength);
    if (header[0] !== 0x58 || header[1] !== 0x63 ||
        header[2] !== 0x75 || header[3] !== 0x72)
        return [];
    if (headerLength < 16)
        throw new Error('Truncated Xcursor header');

    const headerView = new DataView(header.buffer, header.byteOffset, header.byteLength);
    const headerSize = headerView.getUint32(4, true);
    const version = headerView.getUint32(8, true);
    const tableCount = headerView.getUint32(12, true);
    if (headerSize < 16 || headerSize > fileSize || version !== 0x00010000 ||
        tableCount > Math.floor((fileSize - 16) / 12))
        throw new Error('Invalid Xcursor table');

    const tableLength = tableCount * 12;
    const table = tableLength > 0 ? readExact(stream, fileSize, 16, tableLength) : new Uint8Array();
    const tableView = new DataView(table.buffer, table.byteOffset, table.byteLength);
    const records = [];
    const seenSizes = new Set();
    for (let i = 0; i < tableCount; i++) {
        const entryOffset = i * 12;
        if (tableView.getUint32(entryOffset, true) !== IMAGE_CHUNK_TYPE)
            continue;

        const tableSubtype = tableView.getUint32(entryOffset + 4, true);
        if (seenSizes.has(tableSubtype))
            continue;
        const position = tableView.getUint32(entryOffset + 8, true);
        const chunk = readExact(stream, fileSize, position, 36);
        const chunkView = new DataView(chunk.buffer, chunk.byteOffset, chunk.byteLength);

        const chunkHeader = chunkView.getUint32(0, true);
        const chunkType = chunkView.getUint32(4, true);
        const nominalSize = chunkView.getUint32(8, true);
        const width = chunkView.getUint32(16, true);
        const height = chunkView.getUint32(20, true);
        const hotX = chunkView.getUint32(24, true);
        const hotY = chunkView.getUint32(28, true);
        if (chunkHeader < 36 || chunkType !== IMAGE_CHUNK_TYPE || tableSubtype !== nominalSize)
            throw new Error('Invalid Xcursor image chunk');
        seenSizes.add(nominalSize);

        if (nominalSize > 0 && width > 0 && height > 0 && hotX < width && hotY < height) {
            records.push({size: nominalSize, width, height, hotX, hotY});
        }
    }
    return records;
}

function readCursorDirectory(path) {
    const cursors = new Map();
    const directory = Gio.File.new_for_path(path);
    const enumerator = directory.enumerate_children(
        'standard::name,standard::type', Gio.FileQueryInfoFlags.NONE, null);
    try {
        let info;
        while ((info = enumerator.next_file(null)) !== null) {
            if (info.get_file_type() !== Gio.FileType.REGULAR)
                continue;

            const name = info.get_name();
            try {
                const file = Gio.File.new_for_path(joinPath(path, name));
                const stream = file.read(null);
                let records;
                try {
                    const fileSize = stream.query_info('standard::size', null).get_size();
                    records = parseXcursorFile(stream, fileSize);
                } finally {
                    stream.close(null);
                }
                if (records.length > 0)
                    cursors.set(name, records);
            } catch (error) {
                logFailure('cursor-file-read', error);
            }
        }
    } catch (error) {
        logFailure('cursor-directory-read', error);
    } finally {
        try {
            enumerator.close(null);
        } catch (error) {
            logFailure('cursor-directory-close', error);
        }
    }
    return cursors;
}

function readInheritedThemes(path) {
    let keyFile;
    try {
        const indexPath = joinPath(path, 'index.theme');
        if (fileType(indexPath) !== Gio.FileType.REGULAR)
            return [];

        keyFile = new GLib.KeyFile();
        const contents = new TextDecoder().decode(readFile(indexPath));
        const length = new TextEncoder().encode(contents).byteLength;
        keyFile.load_from_data(contents, length, GLib.KeyFileFlags.NONE);
    } catch (error) {
        logFailure('index-theme-read', error);
        return [];
    }

    try {
        return keyFile.get_string('Icon Theme', 'Inherits')
            .split(',').map(name => name.trim()).filter(Boolean);
    } catch (error) {
        return [];
    }
}

export function loadCursorThemeData(themeName, iconRoots = getIconRoots()) {
    const cursors = new Map();
    const queue = [themeName];
    const visited = new Set();

    while (queue.length > 0 && visited.size < 32) {
        const name = queue.shift();
        if (!name || visited.has(name))
            continue;
        visited.add(name);

        for (const root of iconRoots) {
            let themePath;
            try {
                themePath = joinPath(root, name);
                if (fileType(themePath) !== Gio.FileType.DIRECTORY)
                    continue;
            } catch (error) {
                logFailure('theme-path-read', error);
                continue;
            }

            const cursorPath = joinPath(themePath, 'cursors');
            try {
                if (fileType(cursorPath) === Gio.FileType.DIRECTORY) {
                    for (const [cursorName, records] of readCursorDirectory(cursorPath)) {
                        const existing = cursors.get(cursorName) || [];
                        const seenSizes = new Set(existing.map(record => record.size));
                        for (const record of records) {
                            if (!seenSizes.has(record.size)) {
                                existing.push(record);
                                seenSizes.add(record.size);
                            }
                        }
                        if (existing.length > 0)
                            cursors.set(cursorName, existing);
                    }
                }
            } catch (error) {
                logFailure('theme-path-read', error);
            }

            queue.push(...readInheritedThemes(themePath));
        }
    }

    return cursors;
}

export function buildCursorShapeMap(cursors) {
    const claimsByKey = new Map();
    const sizes = new Set();

    for (const [shape, names] of SHAPE_NAMES) {
        for (const name of names) {
            for (const record of cursors.get(name) || []) {
                if (!Number.isInteger(record.size) || record.size <= 0 ||
                    !Number.isInteger(record.hotX) || !Number.isInteger(record.hotY) ||
                    record.hotX < 0 || record.hotY < 0)
                    continue;

                const key = `${record.size}:${record.hotX}:${record.hotY}`;
                const claims = claimsByKey.get(key) || [];
                claims.push({shape, canonical: name === CANONICAL_NAMES[shape]});
                claimsByKey.set(key, claims);
                sizes.add(record.size);
            }
        }
    }

    const map = new Map();
    for (const [key, claims] of claimsByKey) {
        const canonicalClaims = claims.filter(claim => claim.canonical);
        const candidates = canonicalClaims.length > 0 ? canonicalClaims : claims;
        candidates.sort((a, b) => SHAPE_PRIORITY.get(a.shape) - SHAPE_PRIORITY.get(b.shape));
        map.set(key, candidates[0].shape);
    }

    for (const records of cursors.values()) {
        for (const record of records) {
            if (!Number.isInteger(record.size) || record.size <= 0 ||
                !Number.isInteger(record.hotX) || !Number.isInteger(record.hotY) ||
                record.hotX < 0 || record.hotY < 0)
                continue;
            const key = `${record.size}:${record.hotX}:${record.hotY}`;
            if (!map.has(key))
                map.set(key, 'default');
            sizes.add(record.size);
        }
    }

    return {map, sizes: [...sizes].sort((a, b) => a - b)};
}

export function classifyCursorGeometry(width, height, hotX, hotY) {
    if (![width, height, hotX, hotY].every(Number.isFinite) ||
        width <= 0 || height <= 0 || hotX < 0 || hotY < 0 ||
        hotX >= width || hotY >= height)
        return 'default';

    const x = hotX / width;
    const y = hotY / height;

    if (x >= 0.36 && y >= 0.36)
        return 'text';
    if (x >= 0.22 && y < 0.36)
        return 'hand';
    return 'default';
}

export function classifyCursorFromMap(width, height, hotX, hotY, shapeMap) {
    const fallback = () => classifyCursorGeometry(width, height, hotX, hotY);
    if (![width, height, hotX, hotY].every(Number.isFinite) ||
        width <= 0 || height <= 0 || hotX < 0 || hotY < 0 ||
        hotX >= width || hotY >= height)
        return 'default';

    const {map, sizes} = shapeMap;
    if (width === height) {
        const exact = map.get(`${width}:${hotX}:${hotY}`);
        if (exact)
            return exact;
    }

    if (sizes.length > 0) {
        const spriteSize = Math.max(width, height);
        const preferredSize = shapeMap.cursorSize ?? sizes[0];
        const nearestSizes = [...sizes].sort((a, b) =>
            Math.abs(a - spriteSize) - Math.abs(b - spriteSize) ||
            Math.abs(a - preferredSize) - Math.abs(b - preferredSize));
        for (const nominalSize of nearestSizes) {
            const scale = nominalSize / spriteSize;
            const scaledX = Math.round(hotX * scale);
            const scaledY = Math.round(hotY * scale);
            for (let distance = 0; distance <= 1; distance++) {
                let bestShape = null;
                let bestPriority = Infinity;
                for (let deltaX = -distance; deltaX <= distance; deltaX++) {
                    for (let deltaY = -distance; deltaY <= distance; deltaY++) {
                        if (Math.max(Math.abs(deltaX), Math.abs(deltaY)) !== distance)
                            continue;
                        const shape = map.get(`${nominalSize}:${scaledX + deltaX}:${scaledY + deltaY}`);
                        if (shape && SHAPE_PRIORITY.get(shape) < bestPriority) {
                            bestShape = shape;
                            bestPriority = SHAPE_PRIORITY.get(shape);
                        }
                    }
                }
                if (bestShape)
                    return bestShape;
            }
        }
    }

    return fallback();
}

function getThemeCache(snapshot) {
    if (themeCache)
        return themeCache;
    if (!snapshot?.theme)
        return {map: new Map(), sizes: []};

    const cursors = loadCursorThemeData(snapshot.theme);
    themeCache = {...buildCursorShapeMap(cursors), cursorSize: snapshot.cursorSize};
    return themeCache;
}

function sizeResemblesTheme(width, height, sizes) {
    if (sizes.length === 0)
        return true;
    const spriteSize = Math.max(width, height);
    return sizes.some(size => {
        const ratio = spriteSize / size;
        return ratio >= 0.5 && ratio <= 3;
    });
}

export function classifyCursorTracker(tracker) {
    if (!tracker || typeof tracker.get_hot !== 'function' ||
        typeof tracker.get_sprite !== 'function')
        return 'default';

    try {
        const sprite = tracker.get_sprite();
        if (!sprite || typeof sprite.get_width !== 'function' ||
            typeof sprite.get_height !== 'function')
            return 'default';

        const width = sprite.get_width();
        const height = sprite.get_height();
        const [hotX, hotY] = tracker.get_hot();
        const snapshot = initializeSettings();
        if (!snapshot)
            return classifyCursorGeometry(width, height, hotX, hotY);

        const cache = getThemeCache(snapshot);
        if (cache.sizes.length > 0 && !sizeResemblesTheme(width, height, cache.sizes) &&
            !mismatchInvalidated) {
            mismatchInvalidated = true;
            themeCache = null;
            return classifyCursorGeometry(width, height, hotX, hotY);
        }

        return classifyCursorFromMap(width, height, hotX, hotY, cache);
    } catch (error) {
        return 'default';
    }
}
