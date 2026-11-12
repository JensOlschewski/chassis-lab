function G = createGMatrix(jointData, nJoints, matrixType)
    %CREATEGMATRIX Constructs either Gp or Gs matrix based on joint parameters.
    %
    %   This function creates a matrix G (either Gp or Gs) using joint parameters
    %   from the input structure array. It combines the Gi and Gj matrices
    %   for each joint into a larger G matrix, allocating appropriate
    %   positions based on body IDs. The function dynamically calculates the size 
    %   of G based on the number of bodies and the dimensions of each Gi and Gj.
    %
    %   Inputs:
    %       jointData - A structure array, each element representing a joint.
    %                   It should contain fields 'Gi', 'Gj', 'i.bodyID', and 'j.bodyID'.
    %       nJoints   - The number of bodies in the system, used to determine the
    %                   number of columns in G.
    %       matrixType - A string, either 'GDiag' or 'Gs', determining the type of matrix to construct.
    %
    %   Output:
    %       G - The constructed matrix representing combined Gi and Gj for all joints.

    % Determine total number of rows from all Gi matrices
    numRows = sum(cellfun(@(x) size(x, 1), {jointData.Gi}));
    numCols = nJoints * 6;
    G = zeros(numRows, numCols);  % Initialize G

    % Current index in G
    currentRow = 1;

    % Populate G with Gi and Gj for each entry in jointData
    for i = 1:length(jointData)
        iBodyID = jointData(i).i.bodyID;
        jBodyID = jointData(i).j.bodyID;

        % Determine the size of the current Gi and Gj blocks
        numGRows = size(jointData(i).Gi, 1);

        % Calculate the indices for block entries based on matrix type
        if strcmp(matrixType, 'GDiag')
            % Logic for Gp matrix
            if jBodyID > iBodyID
                i_colIndex = 6 * (i - 1) + (1:6);
                G(currentRow:(currentRow+numGRows-1), i_colIndex) = jointData(i).Gj;
            elseif iBodyID > jBodyID
                j_colIndex = 6 * (i - 1) + (1:6);
                G(currentRow:(currentRow+numGRows-1), j_colIndex) = jointData(i).Gi;
            end

        elseif strcmp(matrixType, 'GDiagMod')
            currentColIndices = 6 * (i - 1) + (1:6);
            
            if jBodyID > iBodyID
                currentGMatrix = jointData(i).Gj;
                currentBodyID = jBodyID;
                currentPointName = jointData(i).j.pointName;

            elseif iBodyID > jBodyID
                currentGMatrix = jointData(i).Gi;
                currentBodyID = iBodyID;
                currentPointName = jointData(i).i.pointName;

            end                
                
            % pointName 
            % bodyID = max(iBodyID, jBodyID);


            G(currentRow:(currentRow+numGRows-1), currentColIndices) = currentGMatrix;

            % Check if the pointName exists in any other jointData entries
            if jointData(i).type == "Symmetriebedingung"
                l = 2; 
                currentPointNameArray = [{jointData(i).i.pointName}, {jointData(i).j.pointName}];
                currentBodyIDArray = [{jointData(i).i.bodyID}, {jointData(i).j.bodyID}];
                currentGMatArray = [{jointData(i).Gi}, {jointData(i).Gj}];
                
            else
                l = 1;
                currentPointNameArray = {currentPointName};
                currentBodyIDArray = {currentBodyID};
                currentGMatArray = {currentGMatrix};
                
            end
            % Check if the pointName exists in any other jointData entries 
            for m = 1:l
                currentPointName = currentPointNameArray{m};
                currentBodyID = currentBodyIDArray{m};
                currentGMatrix = currentGMatArray{m};
                for k = 1:length(jointData)

                    if k ~= i
                        if isfield(jointData(k).i, 'pointName') && strcmp(jointData(k).i.pointName, currentPointName) && currentBodyID == max(jointData(k).i.bodyID, jointData(k).j.bodyID)
                            % Point name found in another joint's i structure
                            currentAddColIndices = 6 * (k - 1) + (1:6);
                            G(currentRow:(currentRow+numGRows-1), currentAddColIndices) = currentGMatrix;
                        elseif isfield(jointData(k).j, 'pointName') && strcmp(jointData(k).j.pointName, currentPointName) && currentBodyID == max(jointData(k).i.bodyID, jointData(k).j.bodyID)
                            % Point name found in another joint's j structure
                            currentAddColIndices = 6 * (k - 1) + (1:6);
                            G(currentRow:(currentRow+numGRows-1), currentAddColIndices) = currentGMatrix;
                        end
                    end
                end
            end

        elseif strcmp(matrixType, 'Gs')
            % Logic for Gs matrix
            if iBodyID ~= 0
                i_colIndex = 6 * (iBodyID - 1) + (1:6);
                G(currentRow:(currentRow+numGRows-1), i_colIndex) = jointData(i).Gi;
            end
            if jBodyID ~= 0
                j_colIndex = 6 * (jBodyID - 1) + (1:6);
                G(currentRow:(currentRow+numGRows-1), j_colIndex) = jointData(i).Gj;
            end
        else
            error('Wrong input for G Matrix Type')
        end

        % Update current index for G
        currentRow = currentRow + numGRows;
    end
end
