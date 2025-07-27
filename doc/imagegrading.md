# Image Grading System

The AudioControl cover art API includes an intelligent image grading system that automatically evaluates and scores cover art images based on multiple quality factors. This helps clients select the best available images for their specific needs.

## Overview

The grading system assigns an integer score (the `grade` field) to each cover art image based on three key factors:
- **Provider reputation** (quality and reliability of the source)
- **File size** (indicator of image quality and detail)
- **Image resolution** (pixel dimensions affecting display quality)

Images are automatically sorted by grade in descending order, with the highest quality images appearing first in API responses.

## Grading Criteria

### 1. Provider Quality Scoring (0-3 points)

Different cover art providers are assigned quality scores based on their reputation for high-quality, accurate images:

| Provider | Score | Rationale |
|----------|-------|-----------|
| **Spotify** | +1 point | Good quality images, but limited selection and variety |
| **TheAudioDB** | +2 points | Good quality with wide variety of image types (album covers, artist photos, etc.) |
| **FanArt.tv** | +3 points | Highest quality, curated content with excellent resolution and artistic quality |
| **Local Files** | +0 points | Quality varies significantly depending on source |
| **Other Providers** | +0 points | Default score for unspecified or new providers |

### 2. File Size Scoring (-1 to +1 points)

File size often correlates with image quality, as higher quality images typically require more storage:

| File Size | Score | Rationale |
|-----------|-------|-----------|
| **< 10KB** | -1 point | Very small files are likely low quality, thumbnails, or heavily compressed |
| **10KB - 100KB** | +0 points | Standard file size range for most web images |
| **> 100KB** | +1 point | Large files typically indicate higher quality, less compression |

### 3. Image Resolution Scoring (-2 to +2 points)

Resolution directly impacts the visual quality and usability of images across different display contexts:

| Resolution | Score | Rationale |
|------------|-------|-----------|
| **< 100×100** | -2 points | Very low resolution, poor for most applications |
| **100×100 - 299×299** | -1 point | Low resolution, suitable only for small thumbnails |
| **300×300 - 599×599** | +0 points | Standard resolution for most web applications |
| **600×600 - 999×999** | +1 point | High resolution, good for most display contexts |
| **≥ 1000×1000** | +2 points | Very high resolution, excellent quality for large displays |

## Grade Range and Interpretation

### Typical Score Range: 0-6

The theoretical maximum score is 6 points (FanArt.tv + large file + very high resolution), while the minimum is -2 points (very small, low-resolution image). In practice, most images score between 0-5 points.

### Grade Examples

| Grade | Example Scenario |
|-------|------------------|
| **6** | 1200×1200 FanArt.tv image, >100KB (3+1+2 points) |
| **5** | 1000×1000 local file, >100KB (0+1+2+2 points) |
| **4** | 700×700 TheAudioDB image, >100KB (2+1+1 points) |
| **3** | 600×600 local file, standard size (0+0+1+2 points) |
| **2** | 640×640 Spotify image, standard size (1+0+1 points) |
| **1** | 200×200 Spotify image, small file (1+0-1+1 points) |
| **0** | 50×50 thumbnail, <10KB (-1-2+variable points) |

## Usage in API Responses

### Automatic Sorting

All cover art API endpoints automatically sort images by grade in descending order, ensuring the highest quality images appear first in the response.

### Grade Field

The `grade` field is included in each image object within API responses:

```json
{
  "url": "https://example.com/image.jpg",
  "width": 1000,
  "height": 1000,
  "size_bytes": 234567,
  "format": "JPEG",
  "grade": 4
}
```

### Client Implementation Guidelines

#### Selecting Images by Quality

```javascript
// Get the highest quality image
const bestImage = response.results[0].images[0];

// Filter images by minimum quality threshold
const highQualityImages = response.results
  .flatMap(result => result.images)
  .filter(image => image.grade >= 3);

// Select image based on size requirements
const suitableImage = response.results
  .flatMap(result => result.images)
  .find(image => image.grade >= 2 && image.width >= 300);
```

#### Quality Thresholds

Recommended grade thresholds for different use cases:

| Use Case | Minimum Grade | Rationale |
|----------|---------------|-----------|
| **Large displays/print** | 4+ | Requires high resolution and quality |
| **Standard web display** | 2+ | Good balance of quality and file size |
| **Thumbnails/previews** | 1+ | Basic quality acceptable |
| **Fallback/any image** | 0+ | Accept any available image |

## Implementation Details

### Grading Process

1. **Image Analysis**: When cover art is retrieved, each image is analyzed for dimensions and file size
2. **Score Calculation**: The grading algorithm calculates scores based on provider, size, and resolution
3. **Sorting**: Images within each provider result are sorted by grade (highest first)
4. **Response Assembly**: The sorted results are included in the API response

### Performance Considerations

- Grading is performed during image retrieval and cached for subsequent requests
- The grading process adds minimal overhead to API response times
- Grade calculations are based on readily available metadata (no image content analysis required)

### Future Enhancements

The grading system is designed to be extensible. Future versions may include:
- **Content analysis**: Evaluating image sharpness, color quality, and artistic composition
- **User preferences**: Allowing clients to specify custom weighting for different factors
- **Provider-specific adjustments**: Fine-tuning scores based on observed quality patterns
- **Machine learning**: Using AI to improve quality assessment accuracy

## Backward Compatibility

The `grade` field is optional in API responses, ensuring backward compatibility with existing clients. Clients that don't use grading will continue to work unchanged, while new clients can take advantage of the quality scoring system.

## Related Documentation

- [Cover Art API](api.md#cover-art-api) - Complete API reference for cover art endpoints
- [Caching System](caching.md) - How image metadata and grades are cached
- [Provider Configuration](README.md) - Setting up cover art providers
