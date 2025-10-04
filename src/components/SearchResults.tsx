import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface BoundingBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface SearchResult {
  id: string;
  session_id: string;
  timestamp: number;
  text_snippet: string;
  full_text: string;
  confidence: number;
  bounding_box: BoundingBox;
  frame_path: string | null;
  app_context: string | null;
  relevance_score: number;
}

interface SearchResults {
  results: SearchResult[];
  total_count: number;
  query_time_ms: number;
}

interface SearchQuery {
  query: string;
  filters: {
    session_ids?: string[];
    date_range?: {
      start: number;
      end: number;
    };
    min_confidence?: number;
    app_names?: string[];
  };
  limit: number;
  offset: number;
}

interface SearchResultsProps {
  query: string;
  sessionId?: string;
}

export function SearchResults({ query, sessionId }: SearchResultsProps) {
  const [results, setResults] = useState<SearchResults | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [minConfidence, setMinConfidence] = useState(0.7);
  const [showFilters, setShowFilters] = useState(false);

  const pageSize = 20;

  useEffect(() => {
    if (!query || query.length < 2) {
      setResults(null);
      return;
    }

    const performSearch = async () => {
      setLoading(true);
      setError(null);

      try {
        const searchQuery: SearchQuery = {
          query,
          filters: {
            session_ids: sessionId ? [sessionId] : undefined,
            min_confidence: minConfidence,
          },
          limit: pageSize,
          offset: page * pageSize,
        };

        const searchResults = await invoke<SearchResults>('search_text', { query: searchQuery });
        setResults(searchResults);
      } catch (err) {
        console.error('Search failed:', err);
        setError(err instanceof Error ? err.message : 'Search failed');
      } finally {
        setLoading(false);
      }
    };

    performSearch();
  }, [query, sessionId, page, minConfidence]);

  const formatTimestamp = (timestamp: number) => {
    return new Date(timestamp).toLocaleString();
  };

  const highlightQuery = (text: string, query: string) => {
    const parts = text.split(new RegExp(`(${query})`, 'gi'));
    return (
      <span>
        {parts.map((part, i) =>
          part.toLowerCase() === query.toLowerCase() ? (
            <mark key={i} className="bg-yellow-200 font-semibold">
              {part}
            </mark>
          ) : (
            <span key={i}>{part}</span>
          )
        )}
      </span>
    );
  };

  if (!query || query.length < 2) {
    return (
      <div className="text-center py-12 text-gray-500">
        Enter a search query (at least 2 characters) to find OCR results
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex justify-center items-center py-12">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-500"></div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="bg-red-50 border border-red-200 rounded-lg p-4 text-red-700">
        <p className="font-semibold">Search Error</p>
        <p className="text-sm">{error}</p>
      </div>
    );
  }

  if (!results || results.results.length === 0) {
    return (
      <div className="text-center py-12">
        <svg className="mx-auto h-12 w-12 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M9.172 16.172a4 4 0 015.656 0M9 10h.01M15 10h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <p className="mt-4 text-gray-600">No results found for "{query}"</p>
        <p className="text-sm text-gray-500 mt-2">Try adjusting your search or filters</p>
      </div>
    );
  }

  const totalPages = Math.ceil(results.total_count / pageSize);

  return (
    <div className="space-y-4">
      {/* Search metadata */}
      <div className="flex justify-between items-center pb-4 border-b">
        <div className="text-sm text-gray-600">
          Found <span className="font-semibold">{results.total_count}</span> results in{' '}
          <span className="font-semibold">{results.query_time_ms}ms</span>
        </div>
        <button
          onClick={() => setShowFilters(!showFilters)}
          className="text-sm text-blue-600 hover:text-blue-800"
        >
          {showFilters ? 'Hide' : 'Show'} Filters
        </button>
      </div>

      {/* Filters panel */}
      {showFilters && (
        <div className="bg-gray-50 p-4 rounded-lg space-y-3">
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Minimum Confidence: {(minConfidence * 100).toFixed(0)}%
            </label>
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={minConfidence}
              onChange={(e) => {
                setMinConfidence(parseFloat(e.target.value));
                setPage(0);
              }}
              className="w-full"
            />
          </div>
        </div>
      )}

      {/* Results list */}
      <div className="space-y-3">
        {results.results.map((result) => (
          <div key={result.id} className="bg-white border border-gray-200 rounded-lg p-4 hover:shadow-md transition-shadow">
            <div className="flex justify-between items-start mb-2">
              <div className="flex-1">
                <p className="text-sm text-gray-500">
                  {formatTimestamp(result.timestamp)}
                  {result.app_context && (
                    <span className="ml-2 px-2 py-1 bg-blue-100 text-blue-700 text-xs rounded">
                      {result.app_context}
                    </span>
                  )}
                </p>
              </div>
              <div className="flex items-center space-x-2">
                <span className="text-xs text-gray-500">
                  Confidence: {(result.confidence * 100).toFixed(0)}%
                </span>
                <span className="text-xs text-gray-500">
                  Score: {result.relevance_score.toFixed(2)}
                </span>
              </div>
            </div>

            <div className="text-gray-800 mb-2">
              {highlightQuery(result.text_snippet, query)}
            </div>

            {result.full_text !== result.text_snippet && (
              <details className="text-sm text-gray-600 mt-2">
                <summary className="cursor-pointer text-blue-600 hover:text-blue-800">
                  Show full text
                </summary>
                <p className="mt-2 p-2 bg-gray-50 rounded">{result.full_text}</p>
              </details>
            )}

            <div className="mt-2 text-xs text-gray-500">
              Location: ({result.bounding_box.x}, {result.bounding_box.y}) - {result.bounding_box.width}x
              {result.bounding_box.height}px
            </div>
          </div>
        ))}
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex justify-center items-center space-x-2 pt-4">
          <button
            onClick={() => setPage(Math.max(0, page - 1))}
            disabled={page === 0}
            className="px-4 py-2 border border-gray-300 rounded-lg disabled:opacity-50 disabled:cursor-not-allowed hover:bg-gray-50"
          >
            Previous
          </button>
          <span className="text-sm text-gray-600">
            Page {page + 1} of {totalPages}
          </span>
          <button
            onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
            disabled={page === totalPages - 1}
            className="px-4 py-2 border border-gray-300 rounded-lg disabled:opacity-50 disabled:cursor-not-allowed hover:bg-gray-50"
          >
            Next
          </button>
        </div>
      )}
    </div>
  );
}
