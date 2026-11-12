function f_preload = createBushStructPreloadVec(bushStruct, numBodies, varargin)
    % createBushStructPreloadVec Generates a preload force vector for bush elements.
    %
    % This function computes and accumulates the preload forces for each
    % bush element in a structure array. It allows handling of bush elements 
    % with an optional input that can modify the source of preload data.
    % It transforms local preload forces into global coordinates and accumulates
    % them into a single vector, considering the active state and body IDs of 
    % each element in the bush structure.
    %
    % Inputs:
    %   bushStruct  - A struct array with bush element information, including
    %                 fields like 'bodyID', 'ebasis', and 'preloads'.
    %   numBodies   - The number of bodies in the system, used to define the size
    %                 of the preload vector.
    %   varargin    - Optional input to specify an alternate source for
    %                 preload data.
    %
    % Outputs:
    %   f_preload   - The calculated preload force vector. It is a column vector
    %                 with a length of '6*numBodies', corresponding to the global
    %                 preload forces for each body.
    %
    % The function first determines the source of preload data based on the
    % number of input arguments. It then iterates through each bush element,
    % transforming and accumulating the local preload forces into a global
    % preload vector. This process is conditioned on the active status and
    % body IDs of the elements.

    % Initialize the preload vector with zeros
    f_preload = zeros(6*numBodies, 1);

    % Determine source of preloadCurrentLocal based on nargin
    if nargin == 2
        preloadSource = bushStruct;
    elseif nargin == 3
        preloadSource = varargin{1};
    else
        error('Invalid number of input parameters');
    end

    for i = 1:length(bushStruct)
        if bushStruct(i).active
            preloadCurrentLocal = preloadSource(i).preloadCurrent;
            Rj = createBlkDiag(bushStruct(i).j.ebasis, bushStruct(i).j.ebasis);

            % Process 'i' body
            if bushStruct(i).i.bodyID ~= 0
                f_preload = addToPreload(f_preload, bushStruct(i).i, -preloadCurrentLocal, bushStruct(i).Tvi, Rj);
            end

            % Process 'j' body
            if bushStruct(i).j.bodyID ~= 0
                f_preload = addToPreload(f_preload, bushStruct(i).j, preloadCurrentLocal, bushStruct(i).Tvj, Rj);
            end
        end
    end
end

function f_preload = addToPreload(f_preload, bodyStruct, preloadLocal, T, Rj)
    % Helper function to add preload forces to the global preload vector.
    % It transforms local preload forces into global coordinates and updates
    % the preload vector at indices corresponding to the specific body ID.

    % Calculate global preload at joint
    globalPreloadAtJoint = Rj * preloadLocal;

    % Calculate global preload
    globalPreload = (T)' * globalPreloadAtJoint;

    % Define indices for the body
    rowindex = (bodyStruct.bodyID * 6 - 5): (bodyStruct.bodyID * 6);

    % Add globalPreload to f_preload at the specified indices
    f_preload(rowindex) = f_preload(rowindex) + globalPreload;
end
