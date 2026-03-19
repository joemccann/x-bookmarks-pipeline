# X API Bookmarks Research

> **TL;DR**: There is no way to fetch a user's bookmarks without their explicit OAuth consent. This is by design - bookmarks are private data. The pipeline already implements all possible friction-reduction measures.

## X API v2 Bookmark Endpoint

### GET /2/users/:id/bookmarks

| Attribute | Value |
|-----------|-------|
| Authentication | OAuth 2.0 User Context **ONLY** |
| Scopes required | `bookmark.read`, `tweet.read`, `users.read` |
| Rate limit | 180 requests per 15-minute window (per user) |
| Access level | Basic ($100/mo) or higher |
| Cost | $0.05 per 100 bookmarks |

### Key Constraint: User Context Required

From the [X API documentation](https://developer.x.com/en/docs/twitter-api/tweets/bookmarks/introduction):

> "Bookmarks lookup returns a list of Tweets that **the authenticated user** has bookmarked."
> "This endpoint requires **OAuth 2.0 User Context** authentication."

This means:

1. **Cannot fetch another user's bookmarks** - bookmarks are private by design
2. **User MUST authenticate** - there's no app-only auth path for bookmarks
3. **OAuth 2.0 with PKCE** is required for public clients
4. **User must explicitly grant `bookmark.read` scope**

## Authentication Options Analyzed

### Option 1: OAuth 2.0 with PKCE (Current Implementation)
- ✅ User authenticates once via browser
- ✅ Refresh token allows long-lived access (~6 months if used regularly)
- ❌ User must explicitly authorize bookmark.read scope (one time)

### Option 2: OAuth 1.0a User Context
- ❌ Same requirement - user must authenticate
- ❌ No refresh tokens, more complex signing
- ❌ Not recommended by X

### Option 3: OAuth 2.0 Confidential Client
- Requires server-side secret storage
- ❌ Still requires user to authenticate
- ✅ Can use longer-lived refresh tokens

### Option 4: App-Only Authentication (Bearer Token)
- ❌ **Does NOT work for bookmarks**
- Only works for public data (tweets, user profiles)
- Bookmarks are private, require user context

## Can We Eliminate Auth Friction?

**Answer: No.**

Bookmarks are intentionally private. X's design requires:
1. User consent (OAuth approval)
2. User-scoped token (not app-only)

This is a **privacy feature, not a limitation** we can work around.

## Friction-Reduction Measures (All Implemented)

| Optimization | Status | Implementation |
|--------------|--------|----------------|
| One-time OAuth with refresh tokens | ✅ | `main.rs`: token persistence to `.env` |
| CDP auto-consent click | ✅ | `browser.rs`: auto-clicks "Authorize app" button |
| Token persistence across restarts | ✅ | Refresh token saved to `.env` |
| Token validation caching | ✅ | `x_api_cache.rs`: skip /users/me within 5min |
| Username→ID caching | ✅ | `x_api_cache.rs`: 30-day cache |
| Early termination fetch | ✅ | `fetcher.rs`: stop on N consecutive cached |
| Reduced daemon API calls | ✅ | 25 bookmarks, 1 page, 15min intervals |
| Auto-reauth on token expiry | ✅ | `main.rs`: browser flow + CDP |

## Alternative Approaches (Not Viable)

### 1. Web Scraping
- ❌ Against X Terms of Service
- ❌ Unreliable (HTML changes frequently)
- ❌ Could get account suspended/banned
- ❌ Requires maintaining browser session

### 2. Third-Party Services
- ❌ Would still need user's OAuth token
- ❌ Additional privacy/security concerns
- ❌ Added dependency and cost

### 3. Browser Extension
- Could auto-export bookmarks to local storage
- ❌ Still requires user to install extension
- ❌ User must have browser open
- ❌ Different tech stack

### 4. Mobile App API Proxy
- ❌ Against X Terms of Service
- ❌ Reverse engineering required
- ❌ Unstable, could break anytime

### 5. Twitter Archive Export
- User can request full data export from X
- ❌ Manual process (not automated)
- ❌ Takes 24-48 hours to generate
- ❌ Not real-time

## API Call Optimization (Current State)

### Pricing
- `GET /2/users/:id/bookmarks`: $0.05 per 100 tweets
- `GET /2/users/me`: $0.01 per request
- `GET /2/users/by/username/:username`: $0.01 per request
- OAuth token operations: Free

### Optimizations Implemented

1. **Incremental Fetching**
   - Stop fetching when hitting consecutive cached bookmarks
   - X returns newest first, so early stop = all new content captured
   - Saves entire pages of API calls

2. **Caching Layer** (`x_api_cache.rs`)
   - Username → user_id: 30-day cache
   - Token validation: 5-minute in-memory cache
   - Request budgeting with daily/cycle limits

3. **Daemon Mode Defaults**
   - 25 bookmarks per cycle (vs 100)
   - 1 page per cycle (vs 5)
   - 15-minute intervals (vs 5 minutes)
   - ~90% reduction in API calls

### Cost Comparison

| Mode | Before Optimization | After Optimization |
|------|--------------------|--------------------|
| Single run | ~$0.05-0.25 | ~$0.01-0.05 |
| Daemon (daily) | ~$1.44/day | ~$0.05/day |

## Conclusion

The X API Bookmarks endpoint is designed with privacy as a core requirement. **There is no technical workaround** to eliminate the OAuth consent requirement.

The current implementation already achieves:
- **Minimum friction**: One-time auth with auto-consent
- **Maximum session persistence**: Refresh tokens kept alive by daemon
- **Minimum API costs**: Caching, early termination, reduced limits

### Remaining User Friction

1. First-time setup: User must authorize the app once
2. Token expiry (~6 months): Re-authorization required
3. Scope changes: Re-authorization if app requests new permissions

This friction is **by design** and cannot be eliminated without violating X's Terms of Service.

---

*Last updated: 2025-03-19*
*Research conducted for x-bookmarks-pipeline optimization*
