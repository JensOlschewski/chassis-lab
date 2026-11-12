function BlkDiag = createBlkDiag(varargin)
    % Function to create a block diagonal matrix from a set of input matrices.
    % This custom implementation may offer performance benefits over MATLAB's blkdiag function in some cases.

    % Count the total number of rows and columns needed for the block diagonal matrix
    totalRows = 0;
    totalCols = 0;
    for i = 1:nargin
        [rows, cols] = size(varargin{i});
        totalRows = totalRows + rows;
        totalCols = totalCols + cols;
    end

    % Initialize the block diagonal matrix with zeros
    BlkDiag = zeros(totalRows, totalCols);

    % Fill in the blocks
    currentRow = 1;
    currentCol = 1;
    for i = 1:nargin
        [rows, cols] = size(varargin{i});
        BlkDiag(currentRow:(currentRow+rows-1), currentCol:(currentCol+cols-1)) = varargin{i};
        currentRow = currentRow + rows;
        currentCol = currentCol + cols;
    end
end
