function GlobalCMatrix = createGlobCMat(bushStruct,numJoints)
    %createGlobCMat Creates a global coupling matrix from a given structure array.
    %   GlobalCMatrix = createGlobCMat(bushStruct) returns a global coupling matrix 
    %   based on the 'charStruct.cMatrixCoup' fields of the active elements in bushStruct.
    %
    %   Input:
    %       bushStruct - A structure array with fields 'active', 'jointID', and 'charStruct.cMatrixCoup'.
    %
    %   Output:
    %       GlobalCMatrix - A global coupling matrix of size (length(bushStruct)*6, length(bushStruct)*6).
    
    matrixSize = numJoints * 6;
    GlobalCMatrix = zeros(matrixSize, matrixSize);  % Initialize the global matrix with zeros

    % Create a logical index array from the 'active' field of bushStruct
    activeIndices = [bushStruct.active];

    % Check if any element in bushStruct is active
    if ~any(activeIndices)
        return;  % Return the zero matrix if no elements are active
    end

    % Check for topology if jointID is present no couple bushing else couple bushing
    
    for i = find(activeIndices)
        jointID = bushStruct(i).jointID;
        if jointID ~=0
            rowColIndex = (jointID*6-5):(jointID*6);
            GlobalCMatrix(rowColIndex, rowColIndex) = bushStruct(i).charStruct.cMatrixCoup + GlobalCMatrix(rowColIndex, rowColIndex); 
        else
            iBodyID = bushStruct(i).i.bodyID;
            jBodyID = bushStruct(i).j.bodyID;
            colIndex = (iBodyID*6-5):(iBodyID*6);
            rowIndex = (jBodyID*6-5):(jBodyID*6); 
            GlobalCMatrix(rowIndex, colIndex) = bushStruct(i).charStruct.cMatrixCoup + GlobalCMatrix(rowIndex, colIndex);
        end
    end
end