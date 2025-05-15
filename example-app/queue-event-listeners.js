// This script adds click event listeners to queue items to make them playable
document.addEventListener('DOMContentLoaded', function() {
    // This function will be called after the displayQueue function renders queue items
    function addClickableQueueItems() {
        console.log("Adding click listeners to queue items");
        const queueContainer = document.getElementById('queue-container');
        if (!queueContainer) return;
        
        // Get all queue items
        const queueItems = queueContainer.querySelectorAll('.queue-item');
        
        // Add click event listeners to queue items
        queueItems.forEach(item => {
            item.addEventListener('click', function(event) {
                // Only handle clicks on the item itself, not on buttons inside it
                if (event.target.closest('.queue-action-btn')) {
                    return; // Let the button's own handler take over
                }
                
                const index = parseInt(this.getAttribute('data-index'), 10);
                if (!isNaN(index) && typeof playQueueIndex === 'function') {
                    playQueueIndex(index);
                }
            });
        });
        
        // Get play and remove buttons
        const playButtons = queueContainer.querySelectorAll('.play-track-btn');
        const removeButtons = queueContainer.querySelectorAll('.remove-track-btn');
        
        // Add stopPropagation to all buttons to prevent triggering item click
        playButtons.forEach(button => {
            button.addEventListener('click', function(event) {
                event.stopPropagation();
            });
        });
        
        removeButtons.forEach(button => {
            button.addEventListener('click', function(event) {
                event.stopPropagation();
            });
        });
    }
    
    // Create a MutationObserver to watch for changes to the queue container
    const queueContainer = document.getElementById('queue-container');
    if (queueContainer) {
        // Create an observer instance
        const observer = new MutationObserver(function(mutations) {
            mutations.forEach(function(mutation) {
                if (mutation.type === 'childList') {
                    // Queue content has changed, add click listeners
                    addClickableQueueItems();
                }
            });
        });
        
        // Start observing the queue container for changes
        observer.observe(queueContainer, { childList: true });
    }
});
