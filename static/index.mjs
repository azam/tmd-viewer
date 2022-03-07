const LANG_CODE = document.documentElement.lang || navigator.language;
const DATETIME_FORMAT = (new Intl.DateTimeFormat(LANG_CODE, { dateStyle: 'long', timeStyle: 'short' }));

let VIEWS = ['feeds', 'settings'];
let TAB_IDS = ['feedsTab', 'settingsTab'];
let VIEW_IDS = ['feedsView', 'settingsView']

let currentView = 'feeds';
let feedsState = {
    query: {
        user_name: undefined,
        keyword: undefined,
        has_media_only: undefined,
        since: undefined,
        until: undefined,
        page: 0,
        count: undefined,
    },
    hasPrevious: false,
    hasNext: false,
    showFilter: false,
};

// Utilities

function formatDate(value) {
    // https://blog.webdevsimplified.com/2020-07/relative-time-format/
    return DATETIME_FORMAT.format(value);
}

function isString(value) {
    return (typeof value === 'string' || value instanceof String);
}

function isStringEmpty(value) {
    return !(isString(value) && value.trim() !== '');
}

function encodeForm(obj) {
    return Object.keys(obj)
        .map((key) => key + '=' + encodeURIComponent(obj[key]))
        .join('&');
}

function encodeQuery(params) {
    return Object.keys(params)
        .filter(key => params[key] !== undefined)
        .map(key => encodeURIComponent(key) + '=' + encodeURIComponent(params[key]))
        .join('&');
}

function decodeQuery(query) {
    return Object.fromEntries((new URLSearchParams(query)).entries());
}

function replaceHash(value) {
    // https://stackoverflow.com/questions/4106702/change-hash-without-triggering-a-hashchange-event
    history.replaceState(null, null, document.location.pathname + '#' + value);
}

function pushHash(value) {
    // https://stackoverflow.com/questions/4106702/change-hash-without-triggering-a-hashchange-event
    history.replaceState(null, null, document.location.pathname + '#' + value);
    history.pushState();
}

function gotoHash(value) {
    // This will trigger hashchange
    window.hash = value;
}

// Utilities DOM

function listen(id, evtName, fn) {
    byId(id).addEventListener(evtName, fn, false);
}

function byId(id) {
    return document.getElementById(id);
}

function nest(...elems) {
    if (!elems || elems.length <= 0) return undefined;
    let parent = elems.pop();
    if (parent) {
        while (elems.length > 0) {
            let newParent = elems.pop();
            if (newParent) {
                newParent.appendChild(parent);
                parent = newParent;
            }
        }
    }
    return parent;
}

function append(parent, ...children) {
    if (children) children.forEach(child => { child && parent.appendChild(child); });
    return parent;
}

function clearChild(elem) {
    //e.firstElementChild can be used.
    var child = elem.lastElementChild;
    while (child) {
        elem.removeChild(child);
        child = elem.lastElementChild;
    }
}

function textNode(text) {
    return document.createTextNode(text);
}

function elem(tagName, ...classes) {
    let e = document.createElement(tagName);
    if (classes) classes.forEach(c => {
        if (c) e.classList.add(c);
    });
    return e;
}

function elemId(tagName, id, ...classes) {
    let e = document.createElement(tagName);
    if (classes) classes.forEach(c => {
        if (c) e.classList.add(c);
    });
    if (id) e.id = id;
    return elem;
}

function elemText(tagName, text, ...classes) {
    let e = document.createElement(tagName);
    if (classes) classes.forEach(c => {
        if (c) e.classList.add(c);
    });
    if (isString(text)) {
        e.textContent = text;
    } else if (text instanceof Node) {
        e.appendChild(text);
    }
    return e;
}

function link(url, title, inner) {
    let e = document.createElement('a');
    e.href = url;
    e.title = title;
    if (inner instanceof Node) {
        e.appendChild(inner);
    } else if (isString(inner)) {
        e.textContent = text;
    }
    return e;
}

function addClass(elem, cls) {
    if (elem && elem instanceof Element) {
        elem.classList.add(cls);
    }
}

function removeClass(elem, cls) {
    if (elem && elem instanceof Element) {
        elem.classList.remove(cls);
    }
}

function toggleClass(elem, cls) {
    if (elem && elem instanceof Element) {
        if (elem.classList.contains(cls)) {
            elem.classList.remove(cls);
        } else {
            elem.classList.add(cls);
        }
    }
}

function exclusiveClass(cls, onElem, ...offElems) {
    if (offElems) {
        offElems.forEach(elem => {
            if (elem && elem instanceof Element) elem.classList.remove(cls);
        });
    }
    if (onElem && onElem instanceof Element) {
        onElem.classList.add(cls);
    }
}

function exclusiveNotClass(cls, offElem, ...onElems) {
    if (offElem && offElem instanceof Element) {
        offElem.classList.remove(cls);
    }
    if (onElems) {
        onElems.forEach(elem => {
            if (elem && elem instanceof Element) elem.classList.add(cls);
        });
    }
}

function boolAttr(elem, attr, value) {
    if (value === true) {
        elem.setAttribute(attr, '');
    } else {
        elem.removeAttribute(attr);
    }
}

// Page render

function showView(view) {
    console.log('showView', view);
    if (VIEWS.includes(view)) {
        let tabId = view + 'Tab';
        let viewId = view + 'View';
        let tabElem = byId(tabId);
        let viewElem = byId(viewId);
        exclusiveClass('is-active', tabElem, ...TAB_IDS.filter(id => tabId !== id).map(id => byId(id)));
        exclusiveNotClass('is-hidden', viewElem, ...VIEW_IDS.filter(id => viewId !== id).map(id => byId(id)));
        switch (view) {
            case 'settings':
                showSettings();
                break;
            case 'feeds':
            default:
                showFeeds();
                break;
        }
    }
}

function showSettings() {
    //
}

function feedsHashObject() {
    let query = Object.assign({}, feedsState.query);
    query.page = query.page + 1;
    return query;
}

function updateFeedsStateFromHash() {
    let hash = window.location.hash;
    switch (true) {
        case /^#feeds[\?]?/.test(hash):
            const query = decodeQuery(hash.substring('#feeds'.length));
            feedsState.query.page = (!isNaN(parseInt(query.page)) ? Math.max(1, parseInt(query.page)) : 1) - 1;
            feedsState.query.count = (!isNaN(parseInt(query.count)) && parseInt(query.count) > 0) ? parseInt(query.count) : undefined;
            feedsState.query.user_name = query.user_name ? query.user_name : undefined;
            feedsState.query.keyword = query.keyword ? query.keyword : undefined;
            feedsState.query.has_media_only = (query.has_media_only && query.has_media_only === 'true') ? true : undefined;
            break;
        default:
        // Not a #feeds path
    }
}

function updateFeedsState(evt) {
    let beforeState = Object.assign({}, feedsState);
    beforeState.query = Object.assign({}, feedsState.query);

    // From page input
    let inputPage = byId('feedsPageInput').value;
    feedsState.query.page = (!isNaN(inputPage) && Number.isInteger(inputPage)) ? Math.min(inputPage - 1, 0) : (inputPage - 1);
    let inputUserName = byId('feedsUserNameInput').value;
    feedsState.query.user_name = inputUserName ? inputUserName : undefined;
    let inputKeyword = byId('feedsKeywordInput').value;
    feedsState.query.keyword = inputKeyword ? inputKeyword : undefined;
    let inputHasMediaOnly = byId('feedsHasMediaOnlyInput').checked === true;
    feedsState.query.has_media_only = inputHasMediaOnly ? true : undefined;

    // console.log('updateFeedsState', feedsState);
    return beforeState;
}

function updateFeedsViewState(evt) {
    // console.log('updateFeedsViewState', feedsState);
    byId('feedsPageInput').value = feedsState.query.page + 1;
    byId('feedsUserNameInput').value = feedsState.query.user_name ? feedsState.query.user_name : '';
    byId('feedsKeywordInput').value = feedsState.query.keyword ? feedsState.query.keyword : '';
    byId('feedsHasMediaOnlyInput').checked = feedsState.query.has_media_only === true ? true : false;

    boolAttr(byId('prevFeedsButton'), 'disabled', !feedsState.hasPrevious);
    boolAttr(byId('nextFeedsButton'), 'disabled', !feedsState.hasNext);

    replaceHash('feeds?' + encodeQuery(feedsHashObject()));
}

function onFeedsInputChange(evt) {
    let src = evt.srcElement;
    if (src.id !== 'feedsPageInput') {
        feedsState.query.page = 0;
        byId('feedsPageInput').value = 1;
    }
    let lastState = updateFeedsState(evt);
    console.log('onFeedsInputChange', feedsState, lastState, evt.srcElement);
    if (feedsState.query.page !== lastState.query.page || feedsState.query.user_name !== lastState.query.user_name || feedsState.query.keyword !== lastState.query.keyword || feedsState.query.has_media_only !== lastState.query.has_media_only) {
        fetchFeeds();
    }
}

function showFeeds(evt) {
    // Read input
    updateFeedsStateFromHash();
    fetchFeeds();
}

const TWITTER_URL = 'https://twitter.com';
const LINKIFY_OPTS = {
    formatHref: {
        hashtag: (href) => TWITTER_URL + '/hashtag/' + href.substr(1),
        mention: (href) => TWITTER_URL + href,
    },
    rel: 'noopener noreferrer',
    target: '_blank',
};

function renderFeeds(feeds) {
    let feedsElem = byId('feeds');
    clearChild(feedsElem);
    if (feeds && feeds.length > 0) {
        feeds.forEach(feed => {
            feedsElem.appendChild(renderFeed(feed));
        });
    } else {
        feedsElem.appendChild(renderEmptyFeed());
    }
}

function renderEmptyFeed() {
    return byId('feed-empty-template').content.cloneNode(true);
}

function iconElem(name) {
    let e = elem('span', 'icon', 'material-icons-outlined');
    e.textContent = name;
    return e;
}

function renderFeed(feed) {
    // panel-block:
    // (icon | content | link)
    // content:
    // (<actor> {RT <origin>} | date )
    // (<text>           | media x 4 )
    let isRetweet = feed.retweet ? true : false;
    let f = isRetweet ? feed.retweet : feed;
    let isReply = f.contents && f.contents.startsWith('@');
    let hasMedia = f.media !== undefined && f.media.length > 0;

    let feedElem = byId('feed-template').content.cloneNode(true);

    if (isReply) {
        // addClass(feedElem, 'is-reply');
    }

    // Retweet header
    if (isRetweet) {
        // addClass(feedElem, 'is-retweet');
        let feedHeaderRetweet = feedElem.querySelector('.feed-header-retweet');
        let feedHeaderRetweetText = feedHeaderRetweet.querySelector('.feed-retweet-text a');
        feedHeaderRetweetText.href = TWITTER_URL + '/' + feed.user_name.substring(1);
        let textReplacer = function (elem, text) {
            elem.textContent = text;
        };
        document.l10n.formatValue('feeds-retweet-text', { username: feed.user_name })
            .then(textReplacer.bind(null, feedHeaderRetweetText));
        let feedHeaderRetweetDateTime = feedHeaderRetweet.querySelector('.feed-retweet-datetime');
        feedHeaderRetweetDateTime.textContent = formatDate(new Date(feed.retweet_at * 1000));
    } else {
        let feedHeaderRetweet = feedElem.querySelector('.feed-header-retweet');
        feedHeaderRetweet.parentNode.removeChild(feedHeaderRetweet);
    }

    // Header
    let feedHeaderUsername = feedElem.querySelector('.feed-header-details .feed-username a');
    feedHeaderUsername.textContent = f.user_name;
    feedHeaderUsername.href = TWITTER_URL + '/' + f.user_name.substring(1);
    let feedHeaderDateTime = feedElem.querySelector('.feed-header-details .feed-datetime');
    feedHeaderDateTime.textContent = formatDate(new Date(f.feed_at * 1000));
    let feedHeaderTwitterLink = feedElem.querySelector('.feed-header-details .feed-twitter-url a');
    feedHeaderTwitterLink.href = f.twitter_url;

    // Content
    let feedContent = feedElem.querySelector('.feed-content');
    feedContent.innerHTML = f.contents.replaceAll('\n', '<br/>');
    linkifyElement(feedContent, LINKIFY_OPTS);
    twemoji.parse(feedContent, { base: './static/twemoji-13.1.0/' });

    // Media
    if (hasMedia) {
        let feedMedia = feedElem.querySelector('.feed-media');
        let mediaImageTemplate = byId('feed-media-image-template');
        let mediaVideoTemplate = byId('feed-media-video-template');
        let mediaDeletedTemplate = byId('feed-media-deleted-template');
        f.media.forEach(m => {
            let mediaFileUrl = '/a/media/file/' + m.feed_id + '/' + m.media_id;
            let mediaPreviewUrl = '/a/media/preview/' + m.feed_id + '/' + m.media_id;
            let mediaThumb;
            switch (m.media_type) {
                case 'Image':
                    mediaThumb = m.deleted_at ? mediaDeletedTemplate.content.cloneNode(true) : mediaImageTemplate.content.cloneNode(true);
                    break;
                case 'Video':
                default:
                    mediaThumb = mediaVideoTemplate.content.cloneNode(true);
                    break;
            }
            let mediaLink = mediaThumb.querySelector('.feed-media-link');
            mediaLink.href = mediaFileUrl;
            mediaLink.title = mediaFileUrl;
            if (!m.deleted_at) {
                let thumb = mediaThumb.querySelector('.feed-media-thumbnail');
                thumb.src = m.thumbnail ? ('data:image/jpeg;base64,' + m.thumbnail) : mediaPreviewUrl;
                thumb.alt = mediaFileUrl;
            }
            nest(feedMedia, mediaThumb);
        });
    } else {
        let feedMedia = feedElem.querySelector('.feed-media');
        feedMedia.parentNode.removeChild(feedMedia);
    }

    return feedElem;
}

function fetchFeeds() {
    console.log('fetchFeeds', feedsState.query);
    feedsState.hasNext = false;
    feedsState.hasPrevious = false;
    const query = Object.assign({}, feedsState.query);
    return fetch('/a/feeds?' + encodeQuery(query))
        .then(res => res.json())
        .then(res => {
            if (res.feeds) {
                renderFeeds(res.feeds);
                if (res.feeds.length > 0) {
                    feedsState.hasNext = true;
                }
                if (query.page > 0) {
                    feedsState.hasPrevious = true;
                }
                updateFeedsViewState();
            } else {
                updateFeedsViewState();
            }
        });
}

function nextFeeds() {
    feedsState.query.page++;
    return fetchFeeds();
}

function prevFeeds() {
    if (feedsState.query.page > 0) {
        feedsState.query.page--;
        return fetchFeeds();
    }
    return;
}

async function formPost(url, formObject) {
    console.log('formPost', url, encodeForm(formObject));
    return fetch(url, {
        method: 'POST',
        headers: {
            'Accept': 'application/json',
            'Content-Type': 'application/x-www-form-urlencoded; charset=utf-8',
        },
        body: encodeForm(formObject),
    });
}

async function settingsSetDataDir(evt) {
    const valueElem = byId('settingsSetDataDirValue');
    if (valueElem && valueElem.value) valueElem.classList.add('disabled');
    else return;
    const value = valueElem.value;
    const body = encodeForm({ data_dir: value });
    byId('settingsSetDataDirButton').classList.add('disabled');
    const res = await formPost('/a/set_data_dir', { data_dir: value });
    if (res.status >= 200 && res.status <= 299) {
        byId('settingsSetDataDirButton').classList.remove('disabled');
        byId('settingsSetDataDirValue').classList.remove('disabled');
    } else {
        byId('settingsSetDataDirButton').classList.add('is-danger');
    }
}

async function settingsScan(evt) {
    byId('settingsScanButton').classList.add('disabled');
    const res = await formPost('/a/scan', {});
    if (res.status >= 200 && res.status <= 299) {
        byId('settingsScanButton').classList.remove('disabled');
    } else {
        byId('settingsScanButton').classList.add('is-danger');
    }
}

async function settingsGenerateThumbnails(evt) {
    byId('settingsGenerateThumbnailsButton').classList.add('disabled');
    const res = await formPost('/a/generate_thumbnails', {});
    if (res.status >= 200 && res.status <= 299) {
        byId('settingsGenerateThumbnailsButton').classList.remove('disabled');
    } else {
        byId('settingsGenerateThumbnailsButton').classList.add('is-danger');
    }
}

async function settingsClean(evt) {
    byId('settingsCleanButton').classList.add('disabled');
    const res = await formPost('/a/clean', {});
    if (res.status >= 200 && res.status <= 299) {
        byId('settingsCleanButton').classList.remove('disabled');
    } else {
        byId('settingsCleanButton').classList.add('is-danger');
    }
}

async function settingsState(evt) {
    byId('settingsStateButton').classList.add('disabled');
    const res = await fetch('/a/state');
    if (res.status >= 200 && res.status <= 299) {
        byId('appStateOutput').textContent = JSON.stringify(await res.json(), null, 2);
        byId('settingsStateButton').classList.remove('disabled');
    } else {
        byId('settingsStateButton').classList.add('is-danger');
    }
}


/**
 * /u/username
 */
function routePage(evt) {
    if (evt) {
        console.log('routePage onhashchange', evt.oldURL, evt.newURL);
    }
    const hash = window.location.hash;
    switch (true) {
        case /^#settings[\?]?/.test(hash):
            showView('settings');
            break;
        case /^#feeds[\?]?/.test(hash):
            showView('feeds');
            break;
        default:
            window.location.hash = '#feeds';
    }
}

function switchCss() {
    // byId('themeCssLink').href = preferredTheme() === THEME_DARK ? './static/darkly.bulmaswatch.min.css' : './static/empty.css';
}

function loadPage() {
    console.log('loadPage');

    // darklight
    byId('settingsDarkLightSwitch').checked = (preferredTheme() === THEME_DARK);
    themeCallbacks.push(switchCss);
    themeCallbacks.push((value) => {
        byId('settingsDarkLightSwitch').checked = (value === THEME_DARK);
    });
    applyTheme();

    // Feeds view
    listen('nextFeedsButton', 'click', nextFeeds);
    listen('prevFeedsButton', 'click', prevFeeds);
    listen('feedsPageInput', 'change', onFeedsInputChange);
    listen('feedsUserNameInput', 'change', onFeedsInputChange);
    listen('feedsKeywordInput', 'change', onFeedsInputChange);
    listen('feedsHasMediaOnlyInput', 'change', onFeedsInputChange);

    // Settings view
    listen('settingsDarkLightSwitch', 'change', toggleTheme);
    listen('settingsSetDataDirButton', 'click', settingsSetDataDir);
    listen('settingsScanButton', 'click', settingsScan);
    listen('settingsGenerateThumbnailsButton', 'click', settingsGenerateThumbnails);
    listen('settingsCleanButton', 'click', settingsClean);
    listen('settingsStateButton', 'click', settingsState);

    // Route by hash
    routePage();
}

window.addEventListener('hashchange', routePage, false);
window.addEventListener('load', loadPage, false);
